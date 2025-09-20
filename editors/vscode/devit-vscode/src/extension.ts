import * as vscode from 'vscode';
import { ChildProcess, spawn } from 'child_process';
import * as fs from 'fs';
import * as path from 'path';

interface DevItConfig {
    devitBin?: string;
    mcpdBin?: string;
    netPolicy: 'off' | 'full';
    profile: 'safe' | 'std' | 'danger';
}

interface TimelineEvent {
    summary: string;
    raw: string;
    ts?: string;
}

interface ApprovalRequest {
    tool: string;
    pluginId?: string;
    reason?: string;
    policy?: string;
    phase?: string;
}

interface PanelStateMessage {
    type: 'state';
    events: TimelineEvent[];
    approval: ApprovalRequest | null;
}

type PanelMessage =
    | { type: 'approve'; payload: ApprovalRequest }
    | { type: 'refuse'; payload: ApprovalRequest }
    | { type: 'runRecipe' };

interface McpdOptions {
    netPolicy: 'off' | 'full';
    profile: 'safe' | 'std' | 'danger';
    devitBin: string;
    onExit: (code: number | null, signal: NodeJS.Signals | null) => void;
}

let outputChannel: vscode.OutputChannel;
let panel: vscode.WebviewPanel | undefined;
let journalWatcher: fs.FSWatcher | undefined;
let panelRefreshTimer: NodeJS.Timeout | undefined;
let mcpdClient: McpdClient | undefined;
let mcpdReadyPromise: Promise<void> | undefined;
let mcpdReadyError: Error | undefined;
let workspaceRoot: string | undefined;
let currentConfig: DevItConfig | undefined;
let recipeCodeActionRegistration: vscode.Disposable | undefined;

const APPROVE_COMMAND = 'devit.approveLast';

export function activate(context: vscode.ExtensionContext) {
    outputChannel = vscode.window.createOutputChannel('DevIt');
    workspaceRoot = resolveWorkspaceRoot();
    currentConfig = readDevItConfig();

    if (!workspaceRoot) {
        outputChannel.appendLine('DevIt: no workspace folder detected; extension idle.');
    } else {
        setupMcpd(currentConfig);
    }

    const showPanelDisposable = vscode.commands.registerCommand('devit.showPanel', () => {
        if (!workspaceRoot) {
            vscode.window.showInformationMessage('DevIt: open a workspace to view the timeline.');
            return;
        }
        ensurePanel(workspaceRoot);
    });

    const approveDisposable = vscode.commands.registerCommand(APPROVE_COMMAND, async () => {
        if (!workspaceRoot) {
            vscode.window.showInformationMessage('DevIt: open a workspace to approve requests.');
            return;
        }
        const approval = findLastApprovalRequest(workspaceRoot);
        if (!approval) {
            vscode.window.showInformationMessage('DevIt: no approval-required event found in journal.');
            return;
        }
        const client = await requireMcpdClient();
        if (!client) {
            return;
        }
        try {
            const response = await client.callServerApprove(approval);
            outputChannel.appendLine(`server.approve → ${JSON.stringify(response)}`);
            vscode.window.showInformationMessage(`DevIt: approval sent for ${approval.tool}.`);
        } catch (err) {
            handleError('server.approve failed', err);
        }
    });

    const runRecipeDisposable = vscode.commands.registerCommand('devit.runRecipe', async () => {
        if (!workspaceRoot) {
            vscode.window.showInformationMessage('DevIt: open a workspace to run a recipe.');
            return;
        }
        try {
            const recipes = await listRecipes(workspaceRoot);
            if (!recipes.length) {
                vscode.window.showInformationMessage('DevIt: no recipes discovered.');
                return;
            }
            const picked = await vscode.window.showQuickPick(
                recipes.map((r) => ({
                    label: r.name,
                    description: r.id,
                    detail: r.description ?? '',
                })),
                { placeHolder: 'Select a recipe to dry-run' }
            );
            if (!picked) {
                return;
            }
            const id = picked.description ?? picked.label;
            await runRecipeById(workspaceRoot, id);
        } catch (err) {
            handleError('Recipe run failed', err);
        }
    });

    const runRecipeDirectDisposable = vscode.commands.registerCommand(
        'devit.runRecipeId',
        async (recipeId: string) => {
            if (!workspaceRoot) {
                vscode.window.showInformationMessage('DevIt: open a workspace to run a recipe.');
                return;
            }
            try {
                await runRecipeById(workspaceRoot, recipeId);
            } catch (err) {
                handleError('Recipe run failed', err);
            }
        }
    );

    if (workspaceRoot) {
        recipeCodeActionRegistration = registerRecipeCodeActions(workspaceRoot);
        context.subscriptions.push(recipeCodeActionRegistration);
    }

    const configListener = vscode.workspace.onDidChangeConfiguration((event) => {
        if (!event.affectsConfiguration('devit')) {
            return;
        }
        const next = readDevItConfig();
        if (!configsEqual(currentConfig, next)) {
            currentConfig = next;
            if (workspaceRoot) {
                setupMcpd(next);
                updatePanel(workspaceRoot);
            }
        }
    });

    const workspaceListener = vscode.workspace.onDidChangeWorkspaceFolders(() => {
        const nextRoot = resolveWorkspaceRoot();
        if (nextRoot === workspaceRoot) {
            return;
        }
        workspaceRoot = nextRoot;
        recipeCodeActionRegistration?.dispose();
        recipeCodeActionRegistration = undefined;
        if (workspaceRoot) {
            const config = currentConfig ?? readDevItConfig();
            currentConfig = config;
            setupMcpd(config);
            recipeCodeActionRegistration = registerRecipeCodeActions(workspaceRoot);
            context.subscriptions.push(recipeCodeActionRegistration);
        } else {
            mcpdClient?.dispose();
            mcpdClient = undefined;
        }
    });

    context.subscriptions.push(
        showPanelDisposable,
        approveDisposable,
        runRecipeDisposable,
        runRecipeDirectDisposable,
        configListener,
        workspaceListener,
        outputChannel,
        {
            dispose: () => {
                journalWatcher?.close();
                journalWatcher = undefined;
                if (panelRefreshTimer) {
                    clearInterval(panelRefreshTimer);
                    panelRefreshTimer = undefined;
                }
                mcpdClient?.dispose();
                mcpdClient = undefined;
            },
        }
    );
}

export function deactivate() {
    journalWatcher?.close();
    journalWatcher = undefined;
    if (panelRefreshTimer) {
        clearInterval(panelRefreshTimer);
        panelRefreshTimer = undefined;
    }
    mcpdClient?.dispose();
    mcpdClient = undefined;
    panel?.dispose();
    panel = undefined;
    workspaceRoot = undefined;
}

function resolveWorkspaceRoot(): string | undefined {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders || folders.length === 0) {
        return undefined;
    }
    return folders[0].uri.fsPath;
}

function ensurePanel(root: string) {
    if (!panel) {
        panel = vscode.window.createWebviewPanel(
            'devitPanel',
            'DevIt',
            vscode.ViewColumn.Two,
            {
                enableScripts: true,
                retainContextWhenHidden: true,
            }
        );
        panel.webview.html = renderPanelHtml(panel.webview);
        panel.onDidDispose(() => {
            panel = undefined;
            journalWatcher?.close();
            journalWatcher = undefined;
            if (panelRefreshTimer) {
                clearInterval(panelRefreshTimer);
                panelRefreshTimer = undefined;
            }
        });
        panel.webview.onDidReceiveMessage(async (message: PanelMessage) => {
            if (message.type === 'approve') {
                const client = await requireMcpdClient();
                if (!client) {
                    return;
                }
                try {
                    const response = await client.callServerApprove(message.payload);
                    outputChannel.appendLine(`server.approve → ${JSON.stringify(response)}`);
                    vscode.window.showInformationMessage(
                        `DevIt: approval sent for ${message.payload.tool}.`
                    );
                } catch (err) {
                    handleError('server.approve failed', err);
                }
            } else if (message.type === 'refuse') {
                outputChannel.appendLine(
                    `Refused approval for ${message.payload.tool} (${message.payload.policy ?? 'policy'}).`
                );
                vscode.window.showInformationMessage(
                    `DevIt: refusal logged for ${message.payload.tool}.`
                );
            } else if (message.type === 'runRecipe') {
                void vscode.commands.executeCommand('devit.runRecipe');
            }
        });
    }
    updatePanel(root);
    if (!journalWatcher) {
        const journalPath = path.join(root, '.devit', 'journal.jsonl');
        try {
            journalWatcher = fs.watch(journalPath, { persistent: false }, () => updatePanel(root));
        } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            outputChannel.appendLine(`DevIt: cannot watch journal (${message}).`);
        }
    }
    if (!panelRefreshTimer) {
        panelRefreshTimer = setInterval(() => updatePanel(root), 1000);
    }
}

function updatePanel(root: string) {
    if (!panel) {
        return;
    }
    const events = readJournalEvents(root, 10);
    const approval = findLastApprovalRequest(root) ?? null;
    const message: PanelStateMessage = {
        type: 'state',
        events,
        approval,
    };
    panel.webview.postMessage(message).then(undefined, (err) => {
        const messageText = err instanceof Error ? err.message : String(err);
        outputChannel.appendLine(`DevIt: failed to post panel update (${messageText}).`);
    });
}

function readJournalEvents(root: string, limit: number): TimelineEvent[] {
    const journalPath = path.join(root, '.devit', 'journal.jsonl');
    if (!fs.existsSync(journalPath)) {
        return [];
    }
    try {
        const data = fs.readFileSync(journalPath, 'utf8');
        const lines = data.split(/\r?\n/).filter((line) => line.trim().length > 0);
        const tail = lines.slice(-limit);
        return tail
            .map((line) => {
                try {
                    const parsed = JSON.parse(line);
                    return {
                        summary: summariseEvent(parsed),
                        raw: line,
                        ts: typeof parsed?.ts === 'string' ? parsed.ts : undefined,
                    };
                } catch (err) {
                    return {
                        summary: line.slice(0, 120),
                        raw: line,
                    };
                }
            })
            .reverse();
    } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        outputChannel.appendLine(`DevIt: failed to read journal (${message}).`);
        return [];
    }
}

function summariseEvent(obj: any): string {
    if (obj?.payload?.approval_required) {
        const tool = obj.payload.tool ?? 'unknown';
        const phase = obj.payload.phase ?? 'phase';
        return `⚠️ approval required: ${tool} (${phase})`;
    }
    if (obj?.type === 'tool.result') {
        const tool = obj.payload?.name ?? 'tool';
        return `✅ ${tool}`;
    }
    if (obj?.type === 'tool.error') {
        const reason = obj.payload?.reason ?? 'error';
        return `❌ ${reason}`;
    }
    if (obj?.action) {
        const scope = obj.scope ? ` (${obj.scope})` : '';
        return `📝 ${obj.action}${scope}`;
    }
    if (obj?.tool && obj?.phase) {
        const status = obj.ok === false ? '❌' : '✅';
        return `${status} ${obj.tool} (${obj.phase})`;
    }
    if (obj?.event) {
        return `📝 ${JSON.stringify(obj.event)}`;
    }
    return JSON.stringify(obj);
}

function findLastApprovalRequest(root: string): ApprovalRequest | undefined {
    const journalPath = path.join(root, '.devit', 'journal.jsonl');
    if (!fs.existsSync(journalPath)) {
        return undefined;
    }
    try {
        const data = fs.readFileSync(journalPath, 'utf8');
        const lines = data.split(/\r?\n/).filter((line) => line.trim().length > 0);
        for (let i = lines.length - 1; i >= 0; i -= 1) {
            const line = lines[i];
            try {
                const parsed = JSON.parse(line);
                const payload = parsed?.payload;
                if (payload?.approval_required) {
                    const tool = payload.tool ?? 'unknown';
                    const pluginId = payload.plugin_id as string | undefined;
                    const policy = payload.policy as string | undefined;
                    const phase = payload.phase as string | undefined;
                    return { tool, pluginId, policy, phase };
                }
            } catch {
                continue;
            }
        }
    } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        outputChannel.appendLine(`DevIt: failed to scan journal for approvals (${message}).`);
    }
    return undefined;
}

async function requireMcpdClient(): Promise<McpdClient | undefined> {
    if (!workspaceRoot) {
        vscode.window.showErrorMessage('DevIt: no workspace available.');
        return undefined;
    }
    if (!mcpdClient) {
        vscode.window.showErrorMessage('DevIt: devit-mcpd is not running.');
        return undefined;
    }
    if (mcpdReadyPromise) {
        try {
            await mcpdReadyPromise;
        } catch (err) {
            mcpdReadyError = err instanceof Error ? err : new Error(String(err));
        }
    }
    if (mcpdReadyError) {
        vscode.window.showErrorMessage(
            `DevIt: devit-mcpd unavailable (${mcpdReadyError.message}).`
        );
        return undefined;
    }
    return mcpdClient;
}

interface RecipeSummary {
    id: string;
    name: string;
    description?: string;
}

async function listRecipes(root: string): Promise<RecipeSummary[]> {
    const devitPath = resolveBinary('devit', root, currentConfig?.devitBin);
    const result = await runProcess(devitPath, ['recipe', 'list'], root);
    const stdout = result.stdout.trim();
    if (!stdout) {
        return [];
    }
    try {
        const parsed = JSON.parse(stdout);
        return parsed.recipes ?? [];
    } catch (err) {
        throw new Error('Unexpected JSON from devit recipe list');
    }
}

async function runRecipeDryRun(root: string, id: string): Promise<string> {
    const devitPath = resolveBinary('devit', root, currentConfig?.devitBin);
    const result = await runProcess(devitPath, ['recipe', 'run', id, '--dry-run'], root);
    return result.stdout.trim() || result.stderr.trim();
}

async function runRecipeById(root: string, id: string): Promise<void> {
    const run = await runRecipeDryRun(root, id);
    outputChannel.appendLine(`devit recipe run ${id} --dry-run → ${run}`);
    vscode.window.showInformationMessage(`DevIt: dry-run for ${id} completed.`);
}

async function runProcess(
    bin: string,
    args: string[],
    cwd: string
): Promise<{ stdout: string; stderr: string; code: number }> {
    return new Promise((resolve, reject) => {
        const proc = spawn(bin, args, { cwd, shell: false });
        let stdout = '';
        let stderr = '';
        proc.stdout?.on('data', (chunk) => {
            stdout += chunk.toString();
        });
        proc.stderr?.on('data', (chunk) => {
            stderr += chunk.toString();
        });
        proc.on('error', (err) => reject(err));
        proc.on('close', (code) => {
            if (code === 0) {
                resolve({ stdout, stderr, code: code ?? 0 });
            } else {
                const err = new Error(stderr.trim() || `Process exited with code ${code}`);
                (err as any).stdout = stdout;
                (err as any).stderr = stderr;
                reject(err);
            }
        });
    });
}

function resolveBinary(name: string, root: string, override?: string): string {
    const candidates: string[] = [];
    const ext = process.platform === 'win32' ? '.exe' : '';
    if (override && override.trim()) {
        candidates.push(override.trim());
    }
    const envVar = process.env[`${name.toUpperCase().replace(/[-.]/g, '_')}_BIN`];
    if (envVar) {
        candidates.push(envVar);
    }
    candidates.push(path.join(root, 'target', 'debug', `${name}${ext}`));
    candidates.push(path.join(root, 'target', 'release', `${name}${ext}`));
    candidates.push(`${name}${ext}`);
    for (const candidate of candidates) {
        if (fs.existsSync(candidate)) {
            return candidate;
        }
    }
    return candidates[candidates.length - 1];
}

function registerRecipeCodeActions(root: string): vscode.Disposable {
    const provider = new RecipeCodeActionProvider(root);
    const selector: vscode.DocumentSelector = [
        { language: 'rust', scheme: 'file' },
        { language: 'json', scheme: 'file' },
        { language: 'jsonc', scheme: 'file' },
        { language: 'javascript', scheme: 'file' },
        { language: 'javascriptreact', scheme: 'file' },
        { language: 'typescript', scheme: 'file' },
        { language: 'typescriptreact', scheme: 'file' },
        { language: 'yaml', scheme: 'file' },
        { language: 'toml', scheme: 'file' },
        { language: 'plaintext', scheme: 'file' },
        { pattern: '**/Cargo.toml' },
    ];
    const metadata: vscode.CodeActionProviderMetadata = {
        providedCodeActionKinds: [vscode.CodeActionKind.QuickFix],
    };
    return vscode.languages.registerCodeActionsProvider(selector, provider, metadata);
}

class RecipeCodeActionProvider implements vscode.CodeActionProvider {
    constructor(private readonly root: string) {}

    provideCodeActions(
        document: vscode.TextDocument,
        _range: vscode.Range | vscode.Selection,
        context: vscode.CodeActionContext
    ): vscode.ProviderResult<vscode.CodeAction[]> {
        if (context.only && !context.only.intersects(vscode.CodeActionKind.QuickFix)) {
            return undefined;
        }
        const actions: vscode.CodeAction[] = [];
        const filePath = document.fileName;
        const fileName = path.basename(filePath).toLowerCase();
        const relPath = path.relative(this.root, filePath).toLowerCase();
        const hasWorkflow = this.repoHasWorkflow();

        if (fileName === 'cargo.toml') {
            this.ensureRecipeAction(actions, 'DevIt: add-ci', 'add-ci');
        }

        if (!hasWorkflow && fileName.endsWith('.rs')) {
            this.ensureRecipeAction(actions, 'DevIt: add-ci', 'add-ci');
        }

        if (this.matchesJest(relPath, fileName)) {
            this.ensureRecipeAction(actions, 'DevIt: migrate-jest-vitest', 'migrate-jest-vitest');
        }

        if (!hasWorkflow && this.isCiLanguage(document.languageId)) {
            this.ensureRecipeAction(actions, 'DevIt: add-ci', 'add-ci');
        }

        if (actions.length > 0) {
            outputChannel.appendLine(
                `CodeAction(s): ${actions.length} for ${document.fileName}`
            );
        }
        return actions;
    }

    private matchesJest(relPath: string, fileName: string): boolean {
        if (fileName.startsWith('jest')) {
            return true;
        }
        if (fileName.includes('.test.')) {
            return true;
        }
        return relPath.includes('jest');
    }

    private createRecipeAction(title: string, recipeId: string): vscode.CodeAction {
        const action = new vscode.CodeAction(title, vscode.CodeActionKind.QuickFix);
        action.command = {
            command: 'devit.runRecipeId',
            title,
            arguments: [recipeId],
        };
        action.isPreferred = true;
        return action;
    }

    private ensureRecipeAction(
        actions: vscode.CodeAction[],
        title: string,
        recipeId: string
    ): void {
        const existing = actions.some((action) => action.command?.arguments?.[0] === recipeId);
        if (!existing) {
            actions.push(this.createRecipeAction(title, recipeId));
        }
    }

    private repoHasWorkflow(): boolean {
        const workflowsDir = path.join(this.root, '.github', 'workflows');
        try {
            const entries = fs.readdirSync(workflowsDir, { withFileTypes: true });
            return entries.some((entry) => {
                if (entry.isFile()) {
                    return entry.name.endsWith('.yml') || entry.name.endsWith('.yaml');
                }
                return false;
            });
        } catch {
            return false;
        }
    }

    private isCiLanguage(languageId: string): boolean {
        return languageId === 'yaml' || languageId === 'json' || languageId === 'jsonc';
    }
}

class McpdClient {
    private readonly proc: ChildProcess;
    private readonly pending: Array<{ resolve: (value: any) => void; reject: (err: Error) => void }> = [];
    private buffer = '';
    private disposed = false;

    constructor(
        binary: string,
        cwd: string,
        private readonly out: vscode.OutputChannel,
        options: McpdOptions
    ) {
        const args = ['--yes', '--net', options.netPolicy, '--profile', options.profile];
        const env = { ...process.env };
        env.DEVIT_BIN = options.devitBin;
        this.proc = spawn(binary, args, {
            cwd,
            stdio: ['pipe', 'pipe', 'pipe'],
            env,
        });
        this.proc.stdout?.on('data', (chunk: Buffer) => this.handleStdout(chunk.toString()));
        this.proc.stderr?.on('data', (chunk: Buffer) => {
            this.out.appendLine(`[devit-mcpd] ${chunk.toString().trim()}`);
        });
        this.proc.on('error', (err) => {
            if (!this.disposed) {
                this.failAll(err instanceof Error ? err : new Error(String(err)));
            }
        });
        this.proc.on('exit', (code, signal) => {
            if (!this.disposed) {
                const err = new Error(`devit-mcpd exited (${code ?? 'null'}/${signal ?? 'null'})`);
                this.failAll(err);
            }
            options.onExit(code, signal);
        });
    }

    async initialize(): Promise<void> {
        await this.send({
            type: 'handshake',
            payload: {
                client: 'devit-vscode',
                version: '0.1.0',
            },
        });
        const version = await this.send({ type: 'version' });
        if (version?.type === 'version') {
            const server = version.payload?.server ?? 'unknown';
            this.out.appendLine(`devit-mcpd version: ${server}`);
        }
        const capabilities = await this.send({ type: 'capabilities' });
        if (capabilities?.payload?.tools) {
            this.out.appendLine(
                `devit-mcpd tools: ${capabilities.payload.tools.join(', ')}`
            );
        }
        await this.send({ type: 'ping' });
        await this.send({
            type: 'tool.call',
            payload: { name: 'devit.tool_list', args: {} },
        });
        await this.send({
            type: 'tool.call',
            payload: { name: 'server.policy', args: {} },
        });
    }

    async callServerApprove(request: ApprovalRequest): Promise<any> {
        const payload: any = {
            type: 'tool.call',
            payload: {
                name: 'server.approve',
                args: {
                    name: request.tool,
                    scope: 'once',
                },
            },
        };
        if (request.pluginId) {
            payload.payload.args.plugin_id = request.pluginId;
        }
        if (request.reason) {
            payload.payload.args.reason = request.reason;
        }
        const response = await this.send(payload);
        if (!response) {
            throw new Error('Empty response from server.approve');
        }
        if (response.type === 'tool.error') {
            throw new Error('server.approve rejected');
        }
        return response;
    }

    dispose() {
        this.disposed = true;
        if (!this.proc.killed) {
            this.proc.kill();
        }
        this.pending.splice(0).forEach((pending) => {
            pending.reject(new Error('devit-mcpd disposed'));
        });
    }

    private handleStdout(chunk: string) {
        this.buffer += chunk;
        let idx = this.buffer.indexOf('\n');
        while (idx !== -1) {
            const line = this.buffer.slice(0, idx).trim();
            this.buffer = this.buffer.slice(idx + 1);
            if (line.length > 0) {
                try {
                    const parsed = JSON.parse(line);
                    const pending = this.pending.shift();
                    if (pending) {
                        pending.resolve(parsed);
                    } else {
                        this.out.appendLine(`[devit-mcpd] ${line}`);
                    }
                } catch (err) {
                    this.out.appendLine(`[devit-mcpd] invalid json: ${line}`);
                }
            }
            idx = this.buffer.indexOf('\n');
        }
    }

    private send(message: any): Promise<any> {
        return new Promise((resolve, reject) => {
            if (this.disposed) {
                reject(new Error('devit-mcpd disposed'));
                return;
            }
            if (!this.proc.stdin) {
                reject(new Error('devit-mcpd stdin unavailable'));
                return;
            }
            this.pending.push({ resolve, reject });
            const payload = `${JSON.stringify(message)}\n`;
            this.proc.stdin.write(payload, (err) => {
                if (err) {
                    this.pending.pop();
                    reject(err);
                }
            });
        });
    }

    private failAll(err: Error) {
        while (this.pending.length) {
            const pending = this.pending.shift();
            if (pending) {
                pending.reject(err);
            }
        }
    }
}

function renderPanelHtml(webview: vscode.Webview): string {
    const nonce = createNonce();
    const csp = [
        `default-src 'none'`,
        `img-src ${webview.cspSource} https: data:`,
        `style-src 'nonce-${nonce}' ${webview.cspSource}`,
        `script-src 'nonce-${nonce}'`,
    ].join('; ');
    return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8" />
    <meta http-equiv="Content-Security-Policy" content="${csp}">
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>DevIt Timeline</title>
    <style nonce="${nonce}">
        body { font-family: var(--vscode-font-family); margin: 0; padding: 16px; color: var(--vscode-foreground); background: var(--vscode-editor-background); }
        h1 { font-size: 1.2rem; margin-bottom: 8px; }
        .toolbar { display: flex; gap: 8px; margin-bottom: 12px; }
        button { padding: 4px 12px; border-radius: 4px; border: 1px solid var(--vscode-button-border, transparent); background: var(--vscode-button-background); color: var(--vscode-button-foreground); cursor: pointer; }
        button:disabled { opacity: 0.5; cursor: default; }
        ul { list-style: none; padding-left: 0; margin: 0; display: flex; flex-direction: column; gap: 12px; }
        li { border: 1px solid rgba(255,255,255,0.08); border-radius: 6px; padding: 8px 12px; background: rgba(255,255,255,0.02); }
        .item-summary { font-weight: bold; margin-bottom: 4px; }
        .item-meta { font-size: 0.8rem; opacity: 0.7; margin-bottom: 4px; }
        pre { white-space: pre-wrap; word-break: break-word; margin: 0; font-family: var(--vscode-editor-font-family); font-size: 0.85rem; }
        #status { margin-bottom: 8px; font-size: 0.9rem; opacity: 0.8; }
    </style>
</head>
<body>
    <h1>DevIt Timeline</h1>
    <div id="status">No events yet.</div>
    <div class="toolbar">
        <button id="approve" disabled>Approve</button>
        <button id="refuse" disabled>Refuse</button>
        <button id="runRecipe">Run Recipe…</button>
    </div>
    <ul id="timeline"></ul>
    <script nonce="${nonce}">
        const vscode = acquireVsCodeApi();
        const timeline = document.getElementById('timeline');
        const statusEl = document.getElementById('status');
        const approveBtn = document.getElementById('approve');
        const refuseBtn = document.getElementById('refuse');
        const runBtn = document.getElementById('runRecipe');
        let currentApproval = null;

        approveBtn.addEventListener('click', () => {
            if (!currentApproval) {
                return;
            }
            vscode.postMessage({ type: 'approve', payload: currentApproval });
        });
        refuseBtn.addEventListener('click', () => {
            if (!currentApproval) {
                return;
            }
            vscode.postMessage({ type: 'refuse', payload: currentApproval });
        });
        runBtn.addEventListener('click', () => {
            vscode.postMessage({ type: 'runRecipe' });
        });

        window.addEventListener('message', (event) => {
            const message = event.data;
            if (!message || message.type !== 'state') {
                return;
            }
            const state = message;
            timeline.replaceChildren();
            (state.events || []).forEach((evt) => {
                const li = document.createElement('li');
                const summary = document.createElement('div');
                summary.className = 'item-summary';
                summary.textContent = evt.summary;
                li.appendChild(summary);
                if (evt.ts) {
                    const meta = document.createElement('div');
                    meta.className = 'item-meta';
                    meta.textContent = new Date(evt.ts).toLocaleString();
                    li.appendChild(meta);
                }
                const pre = document.createElement('pre');
                pre.textContent = evt.raw;
                li.appendChild(pre);
                timeline.appendChild(li);
            });

            currentApproval = state.approval;
            const hasApproval = !!currentApproval;
            approveBtn.disabled = !hasApproval;
            refuseBtn.disabled = !hasApproval;
            if (hasApproval) {
                const tool = currentApproval.tool ?? 'unknown';
                const policy = currentApproval.policy ?? 'on_request';
                const phase = currentApproval.phase ?? 'phase';
                statusEl.textContent = 'Approval pending for ' + tool + ' (' + policy + ', ' + phase + ').';
            } else {
                statusEl.textContent = state.events && state.events.length
                    ? 'No pending approval.'
                    : 'No events yet.';
            }
        });
    </script>
</body>
</html>`;
}

function createNonce(): string {
    const possible = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    let text = '';
    for (let i = 0; i < 16; i += 1) {
        text += possible.charAt(Math.floor(Math.random() * possible.length));
    }
    return text;
}

function setupMcpd(config: DevItConfig): void {
    mcpdClient?.dispose();
    mcpdClient = undefined;
    mcpdReadyPromise = undefined;
    mcpdReadyError = undefined;

    if (!workspaceRoot) {
        return;
    }

    try {
        const resolvedDevit = resolveBinary('devit', workspaceRoot, config.devitBin);
        const resolvedMcpd = resolveBinary('devit-mcpd', workspaceRoot, config.mcpdBin);
        const client = new McpdClient(resolvedMcpd, workspaceRoot, outputChannel, {
            netPolicy: config.netPolicy,
            profile: config.profile,
            devitBin: resolvedDevit,
            onExit: (code, signal) => {
                const msg = `devit-mcpd exited (${code ?? 'null'}/${signal ?? 'null'})`;
                outputChannel.appendLine(msg);
                if (!mcpdReadyError) {
                    mcpdReadyError = new Error(msg);
                }
            },
        });
        mcpdClient = client;
        const ready = client
            .initialize()
            .catch((err) => {
                const message = err instanceof Error ? err.message : String(err);
                mcpdReadyError = err instanceof Error ? err : new Error(message);
                outputChannel.appendLine(`DevIt MCPD init failed: ${message}`);
                vscode.window.showWarningMessage(`DevIt MCPD init failed: ${message}`);
                throw err;
            });
        mcpdReadyPromise = ready;
        ready
            .then(() => outputChannel.appendLine('DevIt MCPD: ready'))
            .catch(() => {
                // already logged above
            });
    } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        mcpdReadyError = err instanceof Error ? err : new Error(message);
        outputChannel.appendLine(`DevIt MCPD start failed: ${message}`);
        vscode.window.showWarningMessage(`DevIt MCPD failed to start: ${message}`);
    }
}

function readDevItConfig(): DevItConfig {
    const cfg = vscode.workspace.getConfiguration('devit');
    return {
        devitBin: cfg.get<string>('devitBin')?.trim() || undefined,
        mcpdBin: cfg.get<string>('mcpdBin')?.trim() || undefined,
        netPolicy: (cfg.get<'off' | 'full'>('netPolicy') ?? 'off'),
        profile: (cfg.get<'safe' | 'std' | 'danger'>('profile') ?? 'std'),
    };
}

function configsEqual(a: DevItConfig | undefined, b: DevItConfig | undefined): boolean {
    if (!a || !b) {
        return false;
    }
    return (
        a.devitBin === b.devitBin &&
        a.mcpdBin === b.mcpdBin &&
        a.netPolicy === b.netPolicy &&
        a.profile === b.profile
    );
}

function handleError(prefix: string, err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    outputChannel.appendLine(`${prefix}: ${message}`);
    vscode.window.showErrorMessage(`DevIt: ${prefix} (${message}).`);
}
