import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  AiCatalog,
  AiOptions,
  AppError,
  BatchEvent,
  ComponentEntry,
  ComponentProgress,
  DocumentReport,
  DropAnalysis,
  FieldSuggestion,
  FormField,
  Launch,
  MenuItem,
  NewSavedSignature,
  RecentFile,
  Recipe,
  RecipeRoleEntry,
  RedactMatch,
  RedactOptions,
  RunBatchRequest,
  RunRecipeRequest,
  SavedSignature,
  Settings,
  SmartSplitOptions,
  SplitPart,
  StoredCertificate,
  TextBlock,
  Thumbnail,
  ToolDescriptor,
} from './generated';

export type {
  AiCatalog,
  AiOptions,
  AppError,
  BatchEvent,
  ComponentEntry,
  ComponentProgress,
  DocumentReport,
  DropAnalysis,
  FieldSuggestion,
  FormField,
  Launch,
  MenuItem,
  NewSavedSignature,
  RecentFile,
  Recipe,
  RecipeRoleEntry,
  RedactMatch,
  RedactOptions,
  RunBatchRequest,
  RunRecipeRequest,
  SavedSignature,
  Settings,
  SmartSplitOptions,
  SplitPart,
  StoredCertificate,
  TextBlock,
  Thumbnail,
  ToolDescriptor,
};

/** Name of the event carrying batch progress and results. Matches the Rust event bridge. */
const BATCH_EVENT = 'batch';

function isAppError(value: unknown): value is AppError {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { code?: unknown }).code === 'string' &&
    typeof (value as { message?: unknown }).message === 'string'
  );
}

/**
 * Normalizes anything a command rejects with into an AppError, so callers always have a stable
 * code and a user-facing message. Unknown failures become QW-INTERNAL without raw details.
 */
export function toAppError(error: unknown): AppError {
  if (isAppError(error)) return error;
  return {
    code: 'QW-INTERNAL',
    message: 'Something went wrong. Try again; if it keeps happening, check the log folder.',
  };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toAppError(error);
  }
}

/** Typed commands. Argument keys are camelCase; Tauri maps them to the Rust names. */
export const ipc = {
  appVersion: () => call<string>('app_version'),
  listTools: () => call<ToolDescriptor[]>('list_tools'),
  pickFiles: () => call<string[]>('pick_files'),
  explorerStatus: () => call<boolean>('explorer_status'),
  explorerTools: () => call<string[]>('explorer_tools'),
  explorerEnable: (items: MenuItem[], openTitle: string) =>
    call<void>('explorer_enable', { items, openTitle }),
  explorerDisable: () => call<void>('explorer_disable'),
  takeLaunch: () => call<Launch>('take_launch'),
  pickFolder: () => call<string | null>('pick_folder'),
  recipesList: () => call<Recipe[]>('recipes_list'),
  recipeSave: (recipe: Recipe) => call<Recipe>('recipe_save', { recipe }),
  recipeDelete: (id: string) => call<void>('recipe_delete', { id }),
  recipeRename: (id: string, name: string) => call<Recipe>('recipe_rename', { id, name }),
  recipeExport: (id: string) => call<boolean>('recipe_export', { id }),
  recipeImport: () => call<Recipe | null>('recipe_import'),
  recipeRoles: () => call<RecipeRoleEntry[]>('recipe_roles'),
  folderFiles: (path: string, recursive: boolean, withOffice: boolean) =>
    call<string[]>('folder_files', { folder: { path, recursive }, withOffice }),
  runRecipe: (request: RunRecipeRequest) => call<number>('run_recipe', { request }),
  pickPaths: (kind: 'image' | 'package' | 'certificate' | 'trust' | 'any', multiple: boolean) =>
    call<string[]>('pick_paths', { kind, multiple }),
  classifyInputs: (paths: string[]) => call<DropAnalysis>('classify_inputs', { paths }),
  inspectDocument: (path: string, password: string | null) =>
    call<DocumentReport>('inspect_document', { path, password }),
  renderThumbnail: (path: string, password: string | null, page: number, maxEdge: number) =>
    call<Thumbnail>('render_thumbnail', { path, password, page, maxEdge }),
  recentList: () => call<RecentFile[]>('recent_list'),
  recentAdd: (path: string) => call<void>('recent_add', { path }),
  recentClear: () => call<void>('recent_clear'),
  getSettings: () => call<Settings>('get_settings'),
  saveSettings: (settings: Settings) => call<void>('save_settings', { settings }),
  runBatch: (request: RunBatchRequest) => call<number>('run_batch', { request }),
  cancelJob: (job: number) => call<boolean>('cancel_job', { job }),
  retryFailed: (job: number) => call<void>('retry_failed', { job }),
  openOutput: (path: string) => call<void>('open_output', { path }),
  revealOutput: (path: string) => call<void>('reveal_output', { path }),
  ocrLanguages: () => call<string[]>('ocr_languages'),
  editorFonts: () => call<string[]>('editor_fonts'),
  formFields: (path: string, password: string | null) =>
    call<FormField[]>('form_fields', { path, password }),
  suggestFields: (path: string, password: string | null, page: number) =>
    call<FieldSuggestion[]>('suggest_fields', { path, password, page }),
  textBlocks: (path: string, password: string | null, page: number) =>
    call<TextBlock[]>('text_blocks', { path, password, page }),
  redactMatches: (path: string, password: string | null, options: RedactOptions) =>
    call<RedactMatch[]>('redact_matches', { path, password, options }),
  listCertificates: () => call<StoredCertificate[]>('list_certificates'),
  aiCatalog: () => call<AiCatalog>('ai_catalog'),
  aiModels: (ai: AiOptions) => call<string[]>('ai_models', { ai }),
  smartSplitPreview: (path: string, password: string | null, options: SmartSplitOptions) =>
    call<SplitPart[]>('smart_split_preview', { path, password, options }),
  redactSuggestions: (path: string, password: string | null, options: RedactOptions) =>
    call<string[]>('redact_suggestions', { path, password, options }),
  signaturesList: () => call<SavedSignature[]>('signatures_list'),
  signatureSave: (signature: NewSavedSignature) =>
    call<SavedSignature>('signature_save', { signature }),
  signatureDelete: (id: string) => call<void>('signature_delete', { id }),
  scanAvailable: () => call<boolean>('scan_available'),
  scanAcquire: () => call<string | null>('scan_acquire'),
  saveCapture: (pngBase64: string) => call<string>('save_capture', { pngBase64 }),
  componentsInstalled: () => call<ComponentEntry[]>('components_installed'),
  componentsAvailable: (consent: boolean) => call<ComponentEntry[]>('components_available', { consent }),
  componentInstall: (id: string, consent: boolean) =>
    call<ComponentEntry>('component_install', { id, consent }),
  componentImport: (path: string) => call<ComponentEntry>('component_import', { path }),
  componentRemove: (id: string) => call<void>('component_remove', { id }),
};

/** Name of the event carrying component download progress. Matches the Rust command. */
const COMPONENT_EVENT = 'component-progress';

/** Subscribes to component download progress. Returns a function that stops listening. */
export function onComponentProgress(
  handler: (progress: ComponentProgress) => void,
): Promise<UnlistenFn> {
  return listen<ComponentProgress>(COMPONENT_EVENT, (e) => handler(e.payload));
}

/** Subscribes to launch requests from a second start (an Explorer entry while the app is open). */
export function onLaunch(handler: (launch: Launch) => void): Promise<UnlistenFn> {
  return listen<Launch>('launch', (e) => handler(e.payload));
}

/** Subscribes to batch events. Returns a function that stops listening. */
export function onBatchEvent(handler: (event: BatchEvent) => void): Promise<UnlistenFn> {
  return listen<BatchEvent>(BATCH_EVENT, (e) => handler(e.payload));
}
