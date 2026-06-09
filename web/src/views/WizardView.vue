<script setup lang="ts">
import {
  computed,
  onBeforeUnmount,
  onMounted,
  reactive,
  ref,
  toRaw,
  watch,
} from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";

import {
  ApiError,
  createDraft,
  generateDraft,
  getArtifacts,
  getDraft,
  getDraftTemplate,
  saveDraft,
} from "@/api/client";
import { CONFIG_SETS_API } from "@/api/constants";
import { Icon } from "@iconify/vue";
import AppWorkspaceShell from "@/components/blocks/AppWorkspaceShell.vue";
import type {
  AntiReverseProxyConfig,
  ArtifactDocument,
  BackendNodeConfig,
  BackendNodeType,
  DeploymentConfigPayload,
  DraftDocument,
  NginxConfigPayload,
  PathRewriteConfig,
  StreamMode,
} from "@/api/types";
import FilePreviewPanel from "@/components/blocks/FilePreviewPanel.vue";
import FieldBlock from "@/components/ui/FieldBlock.vue";
import GlassPanel from "@/components/ui/GlassPanel.vue";
import SensitiveInput from "@/components/ui/SensitiveInput.vue";
import StepRail from "@/components/ui/StepRail.vue";
import { useDocumentLocale } from "@/composables/useDocumentLocale";

type StepCard = {
  title: string;
  purpose: string;
  effect: string;
};

type RewritePreview = {
  output: string;
  applied: string[];
  error: string;
};

type RewriteRulePreview = {
  error: string;
  matched: boolean;
  output: string;
};

const AUTOSAVE_DELAY_MS = 20_000;
const DEFAULT_FRONTEND_TEST_PATH = "/media/demo/movie.mkv";
const DEFAULT_BACKEND_TEST_PATH = "/mnt/media/demo/movie.mkv";
const BACKEND_NODE_TYPES: BackendNodeType[] = [
  "Disk",
  "OpenList",
  "DirectLink",
  "googleDrive",
  "WebDav",
];
const PROXY_MODES = ["redirect", "proxy", "accel_redirect"] as const;
const URL_MODES = ["path_join", "query_path", "url_template"] as const;
const LOG_LEVELS = ["trace", "debug", "info", "warn", "error"] as const;
const RESOLVER_PROVIDERS = [
  "none",
  "cloudflare",
  "dnspod",
  "google",
  "aliyun",
  "tencent",
  "custom",
] as const;

const { t, tm } = useI18n();
const route = useRoute();
const router = useRouter();

const steps = tm("wizard.steps") as string[];
const stepCards = tm("wizard.stepCards") as StepCard[];
const finalStepIndex = Math.max(steps.length - 1, 0);

const draft = ref<DraftDocument | null>(null);
const artifacts = ref<ArtifactDocument[]>([]);
const latestConfigSetId = ref("");
const revision = ref(1);
const currentStep = ref(0);
const pending = ref(false);
const saving = ref(false);
const autosaving = ref(false);
const errorMessage = ref("");
const infoMessage = ref("");
const restoredBanner = ref("");
const routeTestInput = ref(DEFAULT_FRONTEND_TEST_PATH);
const nodeRewriteSamples = ref<string[]>([]);
const collapsedArtifacts = reactive<Record<string, boolean>>({});
const autosaveBaseline = ref("");
const autosaveArmed = ref(false);
const toastMessage = ref("");
let toastTimer: number | undefined;
let autosaveTimer: number | undefined;

useDocumentLocale();

const currentStepCard = computed(
  () =>
    stepCards[currentStep.value] ?? {
      title: "",
      purpose: "",
      effect: "",
    },
);

const hasDraft = computed(() => Boolean(draft.value));
const selectedStreamMode = ref<StreamMode>("frontend");
const showFrontend = computed(() => selectedStreamMode.value !== "backend");
const showBackend = computed(() => selectedStreamMode.value !== "frontend");
const isReviewStep = computed(() => currentStep.value === finalStepIndex);

const memoryOptions = computed(() => [
  { value: "low", label: t("wizard.memoryLow") },
  { value: "middle", label: t("wizard.memoryMiddle") },
  { value: "high", label: t("wizard.memoryHigh") },
]);

const userAgentModeOptions = computed(() => [
  { value: "allow", label: t("wizard.uaAllow") },
  { value: "deny", label: t("wizard.uaDeny") },
]);

const backendNodeTypeOptions = computed(() =>
  BACKEND_NODE_TYPES.map((value) => ({
    value,
    label: value,
  })),
);

const proxyModeOptions = computed(() =>
  PROXY_MODES.map((value) => ({
    value,
    label: value,
  })),
);

const urlModeOptions = computed(() =>
  URL_MODES.map((value) => ({
    value,
    label: value,
  })),
);

const logLevelOptions = computed(() =>
  LOG_LEVELS.map((value) => ({
    value,
    label: value,
  })),
);

const resolverProviderOptions = computed(() =>
  RESOLVER_PROVIDERS.map((value) => ({
    value,
    label: t(`wizard.resolverProvider${value}`),
  })),
);

const userAgentRulesText = computed({
  get() {
    const payload = draft.value?.payload.shared.user_agent;
    if (!payload) {
      return "";
    }

    return payload.mode === "allow"
      ? payload.allow_ua.join("\n")
      : payload.deny_ua.join("\n");
  },
  set(value: string) {
    const payload = draft.value?.payload.shared.user_agent;
    if (!payload) {
      return;
    }

    const lines = value
      .split("\n")
      .map((item) => item.trim())
      .filter(Boolean);

    if (payload.mode === "allow") {
      payload.allow_ua = lines;
      payload.deny_ua = [];
    } else {
      payload.deny_ua = lines;
      payload.allow_ua = [];
    }
  },
});

const problematicClientsText = computed({
  get() {
    return draft.value?.payload.backend?.problematic_clients.join("\n") ?? "";
  },
  set(value: string) {
    if (!draft.value?.payload.backend) {
      return;
    }

    draft.value.payload.backend.problematic_clients = value
      .split("\n")
      .map((item) => item.trim())
      .filter(Boolean);
  },
});

const frontendRewritePreview = computed(() =>
  evaluateRewrites(
    routeTestInput.value,
    draft.value?.payload.frontend?.path_rewrites ?? [],
  ),
);

onMounted(async () => {
  const draftId = route.query.draftId;
  if (typeof draftId === "string" && draftId) {
    await loadDraft(draftId, true);
    return;
  }

  await loadLocalTemplate("frontend");
});

onBeforeUnmount(() => {
  if (autosaveTimer !== undefined) {
    window.clearTimeout(autosaveTimer);
  }
  if (toastTimer !== undefined) {
    window.clearTimeout(toastTimer);
  }
});

watch(
  () => draft.value?.stream_mode,
  (value) => {
    if (value) {
      selectedStreamMode.value = value;
    }
  },
  { immediate: true },
);

watch(
  () =>
    draft.value &&
    cloneValue({
      name: draft.value.name,
      payload: draft.value.payload,
    }),
  () => {
    if (!draft.value?.id) {
      return;
    }

    const nextSnapshot = captureAutosaveSnapshot();
    if (!autosaveBaseline.value) {
      autosaveBaseline.value = nextSnapshot;
      return;
    }

    if (nextSnapshot === autosaveBaseline.value) {
      if (autosaveTimer !== undefined) {
        window.clearTimeout(autosaveTimer);
        autosaveTimer = undefined;
      }
      autosaveArmed.value = false;
      return;
    }

    autosaveArmed.value = true;

    if (autosaveTimer !== undefined) {
      window.clearTimeout(autosaveTimer);
    }

    autosaveTimer = window.setTimeout(async () => {
      if (!autosaveArmed.value) {
        return;
      }
      autosaving.value = true;
      try {
        await persistDraft(true);
      } finally {
        autosaving.value = false;
      }
    }, AUTOSAVE_DELAY_MS);
  },
  { deep: true },
);

function syncDraftStreamMode(document: DraftDocument): DraftDocument {
  return {
    ...document,
    payload: {
      ...document.payload,
      stream_mode: document.stream_mode,
    },
  };
}

function cloneValue<T>(value: T): T {
  return JSON.parse(JSON.stringify(toRaw(value))) as T;
}

function captureAutosaveSnapshot() {
  if (!draft.value) {
    return "";
  }

  return JSON.stringify(
    cloneValue({
      name: draft.value.name,
      payload: draft.value.payload,
    }),
  );
}

function resetAutosaveTracking() {
  if (autosaveTimer !== undefined) {
    window.clearTimeout(autosaveTimer);
    autosaveTimer = undefined;
  }

  autosaveArmed.value = false;
  autosaveBaseline.value = captureAutosaveSnapshot();
}

function createPathRewrite(
  pattern = "",
  replacement = "",
  enable = true,
): PathRewriteConfig {
  return {
    enable,
    pattern,
    replacement,
  };
}

function normalizePathRewrite(
  rewrite?: Partial<PathRewriteConfig>,
): PathRewriteConfig {
  return {
    enable: rewrite?.enable ?? false,
    pattern: rewrite?.pattern ?? "",
    replacement: rewrite?.replacement ?? "",
  };
}

function normalizeAntiReverseProxy(
  config?: Partial<AntiReverseProxyConfig>,
): AntiReverseProxyConfig {
  const rawHost = config?.host as unknown;
  let host: string[];
  if (Array.isArray(rawHost)) {
    host = rawHost
      .filter((item): item is string => typeof item === "string")
      .map((item) => item.trim())
      .filter((item) => item.length > 0);
  } else if (typeof rawHost === "string" && rawHost.trim().length > 0) {
    // Backward compatibility with the legacy single-string form.
    host = [rawHost.trim()];
  } else {
    host = [];
  }
  return {
    enable: config?.enable ?? false,
    host,
  };
}

/** Renders the trusted-host list as a comma-separated editable string. */
function hostsToText(hosts: string[]): string {
  return hosts.join(", ");
}

/** Parses comma/whitespace-separated input into a trimmed trusted-host list. */
function textToHosts(text: string): string[] {
  return text
    .split(/[\s,]+/)
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
}

function createNginxPayload(): NginxConfigPayload {
  return {
    frontend: {
      server_name: "stream.example.com",
      ssl_certificate: "",
      ssl_certificate_key: "",
      client_max_body_size: "100M",
      static_location_pattern:
        "\\.(webp|jpg|jpeg|png|gif|ico|css|js|html)$|Images|fonts",
      websocket_location_pattern: "/(socket|embywebsocket)",
    },
    backend: {
      server_name: "stream.example.com",
      ssl_certificate: "",
      ssl_certificate_key: "",
      client_max_body_size: "1G",
      resolver_provider: "none",
      custom_resolvers: "",
      access_log_path: "/var/log/nginx/embystream_access.log",
      error_log_path: "/var/log/nginx/embystream_error.log",
      google_drive_access_log_path: "/var/log/nginx/google_drive_access.log",
    },
  };
}

function createDeploymentPayload(
  streamMode: StreamMode,
): DeploymentConfigPayload {
  const pm2WorkingDirectory =
    streamMode === "frontend"
      ? "/opt/stream-frontend"
      : streamMode === "backend"
        ? "/opt/stream-backend"
        : "/opt/stream";

  return {
    systemd: {
      binary_path: "/usr/bin/embystream",
      working_directory: "/opt/stream",
      config_path: "/opt/stream/config.toml",
    },
    pm2: {
      binary_path: "/usr/bin/embystream",
      working_directory: pm2WorkingDirectory,
      config_path: `${pm2WorkingDirectory}/config.toml`,
      out_file: `${pm2WorkingDirectory}/logs/pm2.out.log`,
      error_file: `${pm2WorkingDirectory}/logs/pm2.err.log`,
    },
  };
}

function normalizeNginxPayload(
  payload: Partial<NginxConfigPayload> | undefined,
): NginxConfigPayload {
  const fallback = createNginxPayload();
  return {
    frontend: {
      ...fallback.frontend,
      ...payload?.frontend,
    },
    backend: {
      ...fallback.backend,
      ...payload?.backend,
      resolver_provider:
        payload?.backend?.resolver_provider ??
        fallback.backend.resolver_provider,
      custom_resolvers:
        payload?.backend?.custom_resolvers ?? fallback.backend.custom_resolvers,
    },
  };
}

function normalizeDeploymentPayload(
  streamMode: StreamMode,
  payload: Partial<DeploymentConfigPayload> | undefined,
): DeploymentConfigPayload {
  const fallback = createDeploymentPayload(streamMode);
  return {
    systemd: {
      ...fallback.systemd,
      ...payload?.systemd,
    },
    pm2: {
      ...fallback.pm2,
      ...payload?.pm2,
    },
  };
}

function mapBackendNodeType(type: string): BackendNodeType {
  return BACKEND_NODE_TYPES.find((value) => value === type) ?? "Disk";
}

function createBackendNode(type: BackendNodeType): BackendNodeConfig {
  const base: BackendNodeConfig = {
    name: type,
    backend_type: type,
    pattern: "",
    base_url: "http://127.0.0.1",
    port: "60002",
    path: "",
    priority: 0,
    proxy_mode: "proxy",
    client_speed_limit_kbs: 0,
    client_burst_speed_kbs: 0,
    path_rewrites: [createPathRewrite("", "", false)],
    anti_reverse_proxy: {
      enable: false,
      host: [],
    },
    disk: null,
    open_list: null,
    direct_link: null,
    google_drive: null,
    webdav: null,
  };

  switch (type) {
    case "OpenList":
      return {
        ...base,
        name: "MyOpenList",
        pattern: "/openlist/.*",
        base_url: "http://alist.example.com",
        port: "5244",
        path: "/openlist",
        proxy_mode: "redirect",
        path_rewrites: [createPathRewrite("^/openlist(/.*)$", "$1", false)],
        open_list: {
          base_url: "http://alist.example.com",
          port: "",
          token: "",
        },
      };
    case "DirectLink":
      return {
        ...base,
        name: "CloudDrive",
        pattern: "/cloud/.*",
        base_url: "https://cloud.example.com",
        port: "443",
        path: "/cloud",
        proxy_mode: "redirect",
        path_rewrites: [
          createPathRewrite(
            "^/cloud(/.*)$",
            "https://cdn.example.com$1",
            false,
          ),
        ],
        direct_link: {
          user_agent: "Mozilla/5.0 (MockClient)",
        },
      };
    case "googleDrive":
      return {
        ...base,
        name: "GoogleDriveMedia",
        pattern: "/gdrive/.*",
        base_url: "https://www.googleapis.com",
        port: "443",
        proxy_mode: "proxy",
        path_rewrites: [createPathRewrite("^/gdrive(/.*)$", "$1", false)],
        google_drive: {
          node_uuid: "google_drive_node_a",
          client_id: "",
          client_secret: "",
          drive_id: "",
          drive_name: "SharedMedia",
          access_token: "",
          refresh_token: "",
        },
      };
    case "WebDav":
      return {
        ...base,
        name: "RcloneWebDav",
        pattern: "/rclone/.*",
        base_url: "http://127.0.0.1",
        port: "60005",
        proxy_mode: "accel_redirect",
        path_rewrites: [createPathRewrite("^/rclone(/.*)$", "$1", false)],
        webdav: {
          url_mode: "path_join",
          node_uuid: "webdav_node_a",
          query_param: "path",
          url_template: "",
          username: "",
          password: "",
          user_agent: "",
        },
      };
    case "Disk":
    default:
      return {
        ...base,
        name: "LocalDisk",
        pattern: "/mnt/media/.*",
        path_rewrites: [
          createPathRewrite("^/mnt/media(/.*)$", "/media$1", false),
        ],
        disk: {
          description: "",
        },
      };
  }
}

function normalizeBackendNode(node: BackendNodeConfig): BackendNodeConfig {
  const fallback = createBackendNode(mapBackendNodeType(node.backend_type));

  return {
    ...fallback,
    ...node,
    backend_type: mapBackendNodeType(node.backend_type),
    priority: Number(node.priority ?? 0),
    proxy_mode: node.proxy_mode ?? fallback.proxy_mode,
    client_speed_limit_kbs: Number(node.client_speed_limit_kbs ?? 0),
    client_burst_speed_kbs: Number(node.client_burst_speed_kbs ?? 0),
    path_rewrites: (node.path_rewrites ?? fallback.path_rewrites).map(
      normalizePathRewrite,
    ),
    anti_reverse_proxy: normalizeAntiReverseProxy(node.anti_reverse_proxy),
    disk: node.disk ?? fallback.disk,
    open_list: node.open_list ?? fallback.open_list,
    direct_link: node.direct_link ?? fallback.direct_link,
    google_drive: node.google_drive ?? fallback.google_drive,
    webdav: node.webdav
      ? {
          ...fallback.webdav,
          ...node.webdav,
        }
      : fallback.webdav,
  };
}

function normalizeDraftDocument(document: DraftDocument): DraftDocument {
  const normalized = syncDraftStreamMode({
    ...document,
    payload: {
      ...document.payload,
      frontend: document.payload.frontend
        ? {
            ...document.payload.frontend,
            path_rewrites: (document.payload.frontend.path_rewrites ?? []).map(
              normalizePathRewrite,
            ),
            anti_reverse_proxy: normalizeAntiReverseProxy(
              document.payload.frontend.anti_reverse_proxy,
            ),
          }
        : null,
      backend: document.payload.backend
        ? {
            ...document.payload.backend,
            problematic_clients:
              document.payload.backend.problematic_clients ?? [],
          }
        : null,
      backend_nodes: (document.payload.backend_nodes ?? []).map(
        normalizeBackendNode,
      ),
      nginx: normalizeNginxPayload(document.payload.nginx),
      deployment: normalizeDeploymentPayload(
        document.stream_mode,
        document.payload.deployment,
      ),
    },
  });

  return normalized;
}

function buildLocalDraft(
  streamMode: StreamMode,
  payload: DraftDocument["payload"],
): DraftDocument {
  return normalizeDraftDocument({
    id: "",
    name: "",
    status: "draft",
    stream_mode: streamMode,
    payload,
    updated_at: "",
  });
}

async function loadDraft(draftId: string, restored: boolean) {
  errorMessage.value = "";
  const response = await getDraft(draftId);
  draft.value = normalizeDraftDocument(response.draft);
  selectedStreamMode.value = response.draft.stream_mode;
  revision.value = 1;
  restoredBanner.value = restored
    ? t("wizard.restoreBanner", { name: response.draft.name })
    : "";
  resetAutosaveTracking();
}

async function startDraft(streamMode: StreamMode) {
  errorMessage.value = "";
  const previousStreamMode = selectedStreamMode.value;
  selectedStreamMode.value = streamMode;

  try {
    await ensureDraft(streamMode);
  } catch (error) {
    selectedStreamMode.value = previousStreamMode;
    throw error;
  }
}

async function loadLocalTemplate(streamMode: StreamMode) {
  pending.value = true;
  errorMessage.value = "";
  infoMessage.value = "";

  try {
    const response = await getDraftTemplate(streamMode);
    draft.value = buildLocalDraft(streamMode, {
      ...response.payload,
      stream_mode: streamMode,
    });
    selectedStreamMode.value = streamMode;
    artifacts.value = [];
    latestConfigSetId.value = "";
    routeTestInput.value = DEFAULT_FRONTEND_TEST_PATH;
    nodeRewriteSamples.value = [];
    resetAutosaveTracking();
  } catch (_error) {
    errorMessage.value = "";
  } finally {
    pending.value = false;
  }
}

async function ensureDraft(streamMode: StreamMode) {
  if (pending.value) {
    return;
  }

  if (!draft.value) {
    await loadLocalTemplate(streamMode);
    return;
  }

  if (draft.value.stream_mode === streamMode) {
    return;
  }

  await switchDraftStreamMode(streamMode);
}

async function switchDraftStreamMode(streamMode: StreamMode) {
  if (!draft.value) {
    return;
  }

  pending.value = true;
  errorMessage.value = "";
  infoMessage.value = "";

  try {
    const response = await getDraftTemplate(streamMode);
    const nextPayload = {
      ...response.payload,
      stream_mode: streamMode,
    };
    const currentDraft = draft.value;

    nextPayload.shared = cloneValue(currentDraft.payload.shared);

    if (nextPayload.frontend && currentDraft.payload.frontend) {
      nextPayload.frontend = cloneValue(currentDraft.payload.frontend);
    }

    if (nextPayload.backend && currentDraft.payload.backend) {
      nextPayload.backend = cloneValue(currentDraft.payload.backend);
    }

    if (currentDraft.payload.backend_nodes.length > 0) {
      nextPayload.backend_nodes = cloneValue(
        currentDraft.payload.backend_nodes,
      );
    }

    draft.value = normalizeDraftDocument({
      ...currentDraft,
      stream_mode: streamMode,
      payload: nextPayload,
    });
    selectedStreamMode.value = streamMode;
    artifacts.value = [];
    latestConfigSetId.value = "";
    currentStep.value = 0;
    nodeRewriteSamples.value = [];
    resetAutosaveTracking();
  } catch (_error) {
    errorMessage.value = "";
  } finally {
    pending.value = false;
  }
}

async function ensureServerDraft() {
  if (!draft.value) {
    return null;
  }

  if (draft.value.id) {
    return draft.value;
  }

  try {
    const response = await createDraft({
      name: draft.value.name,
      stream_mode: draft.value.stream_mode,
    });

    draft.value = normalizeDraftDocument({
      ...draft.value,
      id: response.draft.id,
      name: response.draft.name,
      status: response.draft.status,
      stream_mode: response.draft.stream_mode,
      updated_at: response.draft.updated_at,
    });
    resetAutosaveTracking();
    return draft.value;
  } catch (error) {
    throw error instanceof ApiError
      ? error
      : new ApiError(500, "draft_create_failed", t("errors.draftCreateFailed"));
  }
}

async function persistDraft(fromAutosave = false, createIfNeeded = false) {
  if (!draft.value) {
    return false;
  }

  if (!fromAutosave) {
    saving.value = true;
  }

  errorMessage.value = "";
  try {
    const persistedDraft = createIfNeeded
      ? await ensureServerDraft()
      : draft.value;
    if (!persistedDraft?.id) {
      if (!fromAutosave) {
        infoMessage.value = t("wizard.deferDraftCreation");
      }
      return false;
    }

    const response = await saveDraft(persistedDraft.id, {
      name: draft.value.name,
      payload: draft.value.payload,
      client_revision: revision.value,
    });
    revision.value = response.draft.server_revision;
    draft.value = {
      ...draft.value,
      id: response.draft.id,
      updated_at: response.draft.updated_at,
    };
    resetAutosaveTracking();
    infoMessage.value = t("wizard.savedAt", {
      timestamp: response.draft.updated_at,
    });
    return true;
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : t("errors.draftSaveFailed");
    return false;
  } finally {
    saving.value = false;
  }
}

async function generate() {
  if (!draft.value) {
    return;
  }

  pending.value = true;
  errorMessage.value = "";
  infoMessage.value = "";

  try {
    autosaveArmed.value = false;
    const persisted = await persistDraft(false, true);
    if (!persisted || !draft.value.id) {
      return;
    }

    const response = await generateDraft(draft.value.id);
    latestConfigSetId.value = response.config_set.id;
    const artifactResponse = await getArtifacts(response.config_set.id);
    artifacts.value = artifactResponse.items;
    infoMessage.value = t("wizard.generatedCount", {
      count: artifactResponse.items.length,
    });
    currentStep.value = finalStepIndex;
    await router.push({ name: "config-sets" });
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : t("errors.generationFailed");
  } finally {
    pending.value = false;
  }
}

function nextStep() {
  if (
    currentStep.value === 5 &&
    showBackend.value &&
    (draft.value?.payload.backend_nodes.length ?? 0) === 0
  ) {
    showToast(t("wizard.backendNodeRequired"));
    return;
  }

  if (currentStep.value < finalStepIndex) {
    currentStep.value += 1;
  }
}

function previousStep() {
  if (currentStep.value > 0) {
    currentStep.value -= 1;
  }
}

function showToast(message: string) {
  toastMessage.value = message;
  if (toastTimer !== undefined) {
    window.clearTimeout(toastTimer);
  }
  toastTimer = window.setTimeout(() => {
    toastMessage.value = "";
    toastTimer = undefined;
  }, 2800);
}

function toggleArtifact(fileName: string) {
  collapsedArtifacts[fileName] = !collapsedArtifacts[fileName];
}

function isStreamModeActive(mode: StreamMode) {
  return selectedStreamMode.value === mode;
}

function addFrontendRewrite() {
  draft.value?.payload.frontend?.path_rewrites.push(createPathRewrite());
}

function removeFrontendRewrite(index: number) {
  draft.value?.payload.frontend?.path_rewrites.splice(index, 1);
}

function addBackendNode(type: BackendNodeType) {
  draft.value?.payload.backend_nodes.push(createBackendNode(type));
}

function removeBackendNode(index: number) {
  draft.value?.payload.backend_nodes.splice(index, 1);
  nodeRewriteSamples.value.splice(index, 1);
}

function addNodeRewrite(nodeIndex: number) {
  draft.value?.payload.backend_nodes[nodeIndex].path_rewrites.push(
    createPathRewrite(),
  );
}

function removeNodeRewrite(nodeIndex: number, rewriteIndex: number) {
  draft.value?.payload.backend_nodes[nodeIndex].path_rewrites.splice(
    rewriteIndex,
    1,
  );
}

function evaluateRewrites(
  samplePath: string,
  rewrites: PathRewriteConfig[],
): RewritePreview {
  if (!samplePath.trim()) {
    return {
      output: "",
      applied: [],
      error: "",
    };
  }

  let output = samplePath.trim();
  const applied: string[] = [];

  for (const rewrite of rewrites) {
    if (!rewrite.enable || !rewrite.pattern || !rewrite.replacement) {
      continue;
    }

    try {
      const regex = new RegExp(rewrite.pattern);
      if (!regex.test(output)) {
        continue;
      }

      const nextValue = output.replace(regex, rewrite.replacement);
      applied.push(`${output} -> ${nextValue}`);
      output = nextValue;
    } catch (error) {
      return {
        output,
        applied,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }

  return {
    output,
    applied,
    error: "",
  };
}

function previewSingleRewrite(
  samplePath: string,
  rewrite: PathRewriteConfig,
): RewriteRulePreview {
  if (!samplePath.trim() || !rewrite.pattern.trim()) {
    return {
      error: "",
      matched: false,
      output: "",
    };
  }

  try {
    const regex = new RegExp(rewrite.pattern);
    const matched = regex.test(samplePath);

    return {
      error: "",
      matched,
      output: matched
        ? samplePath.replace(regex, rewrite.replacement || samplePath)
        : "",
    };
  } catch (error) {
    return {
      error: error instanceof Error ? error.message : String(error),
      matched: false,
      output: "",
    };
  }
}

function frontendRewriteRulePreview(rewrite: PathRewriteConfig) {
  return previewSingleRewrite(routeTestInput.value, rewrite);
}

function getNodeRewriteSample(index: number, node: BackendNodeConfig) {
  return (
    nodeRewriteSamples.value[index] || node.pattern || DEFAULT_BACKEND_TEST_PATH
  );
}

function backendNodeRewritePreview(index: number, node: BackendNodeConfig) {
  return evaluateRewrites(
    getNodeRewriteSample(index, node),
    node.path_rewrites,
  );
}

function backendNodeRewriteRulePreview(
  index: number,
  node: BackendNodeConfig,
  rewrite: PathRewriteConfig,
) {
  return previewSingleRewrite(getNodeRewriteSample(index, node), rewrite);
}

function enabledRewriteCount(rewrites: PathRewriteConfig[]) {
  return rewrites.filter(
    (rewrite) => rewrite.enable && rewrite.pattern && rewrite.replacement,
  ).length;
}
</script>

<template>
  <AppWorkspaceShell
    :body="t('wizard.body')"
    :eyebrow="t('wizard.eyebrow')"
    :title="t('wizard.title')"
  >
    <template #hero-actions>
      <button
        class="workspace-shell__hero-action workspace-shell__hero-action--secondary"
        type="button"
        @click="router.push({ name: 'config-sets' })"
      >
        <Icon icon="ph:arrow-left" width="16" />
        {{ t("wizard.backToConfigs") }}
      </button>
    </template>

    <section class="wizard-stage__grid">
      <Transition name="toast-pop">
        <GlassPanel v-if="toastMessage" class="wizard-toast" tone="warm">
          <Icon icon="ph:warning-circle" width="18" />
          <p>{{ toastMessage }}</p>
        </GlassPanel>
      </Transition>

      <GlassPanel class="wizard-stage__editor" tone="warm">
        <div
          v-if="restoredBanner || autosaving || errorMessage || infoMessage"
          class="wizard-stage__status"
        >
          <p v-if="restoredBanner" class="wizard-stage__note">
            {{ restoredBanner }}
          </p>
          <p v-if="autosaving" class="wizard-stage__note">
            {{ t("wizard.autosaving") }}
          </p>
          <p
            v-if="errorMessage"
            class="wizard-stage__note wizard-stage__note--error"
          >
            {{ errorMessage }}
          </p>
          <p
            v-else-if="infoMessage"
            class="wizard-stage__note wizard-stage__note--info"
          >
            {{ infoMessage }}
          </p>
        </div>

        <StepRail
          :current-effect="currentStepCard.effect"
          :current-purpose="currentStepCard.purpose"
          :current-step="currentStep"
          :current-title="currentStepCard.title"
          :steps="steps"
          :title="t('wizard.stepRailTitle')"
          @select="currentStep = Math.min($event, finalStepIndex)"
        />

        <template v-if="!hasDraft">
          <div class="mode-grid">
            <button
              :aria-pressed="isStreamModeActive('frontend')"
              :class="{ active: isStreamModeActive('frontend') }"
              class="mode-card"
              :disabled="pending"
              type="button"
              @click="startDraft('frontend')"
            >
              <span class="mode-card__check" aria-hidden="true">
                <Icon
                  v-if="isStreamModeActive('frontend')"
                  class="mode-card__check-icon"
                  icon="ph:check-bold"
                  width="15"
                />
              </span>
              <span class="mode-card__title">{{ t("modes.frontend") }}</span>
              <span class="mode-card__body">{{
                t("wizard.modeFrontendBody")
              }}</span>
            </button>
            <button
              :aria-pressed="isStreamModeActive('backend')"
              :class="{ active: isStreamModeActive('backend') }"
              class="mode-card"
              :disabled="pending"
              type="button"
              @click="startDraft('backend')"
            >
              <span class="mode-card__check" aria-hidden="true">
                <Icon
                  v-if="isStreamModeActive('backend')"
                  class="mode-card__check-icon"
                  icon="ph:check-bold"
                  width="15"
                />
              </span>
              <span class="mode-card__title">{{ t("modes.backend") }}</span>
              <span class="mode-card__body">{{
                t("wizard.modeBackendBody")
              }}</span>
            </button>
            <button
              :aria-pressed="isStreamModeActive('dual')"
              :class="{ active: isStreamModeActive('dual') }"
              class="mode-card"
              :disabled="pending"
              type="button"
              @click="startDraft('dual')"
            >
              <span class="mode-card__check" aria-hidden="true">
                <Icon
                  v-if="isStreamModeActive('dual')"
                  class="mode-card__check-icon"
                  icon="ph:check-bold"
                  width="15"
                />
              </span>
              <span class="mode-card__title">{{ t("modes.dual") }}</span>
              <span class="mode-card__body">{{
                t("wizard.modeDualBody")
              }}</span>
            </button>
          </div>
        </template>

        <template v-else-if="draft">
          <Transition mode="out-in" name="step-panel">
            <div :key="currentStep">
              <div v-if="currentStep === 0" class="choice-list">
                <button
                  :aria-pressed="isStreamModeActive('frontend')"
                  :class="{ active: isStreamModeActive('frontend') }"
                  class="mode-card"
                  :disabled="pending"
                  type="button"
                  @click="startDraft('frontend')"
                >
                  <span class="mode-card__check" aria-hidden="true">
                    <Icon
                      v-if="isStreamModeActive('frontend')"
                      class="mode-card__check-icon"
                      icon="ph:check-bold"
                      width="15"
                    />
                  </span>
                  <span class="mode-card__title">{{
                    t("modes.frontend")
                  }}</span>
                  <span class="mode-card__body">{{
                    t("wizard.modeFrontendBody")
                  }}</span>
                </button>
                <button
                  :aria-pressed="isStreamModeActive('backend')"
                  :class="{ active: isStreamModeActive('backend') }"
                  class="mode-card"
                  :disabled="pending"
                  type="button"
                  @click="startDraft('backend')"
                >
                  <span class="mode-card__check" aria-hidden="true">
                    <Icon
                      v-if="isStreamModeActive('backend')"
                      class="mode-card__check-icon"
                      icon="ph:check-bold"
                      width="15"
                    />
                  </span>
                  <span class="mode-card__title">{{ t("modes.backend") }}</span>
                  <span class="mode-card__body">{{
                    t("wizard.modeBackendBody")
                  }}</span>
                </button>
                <button
                  :aria-pressed="isStreamModeActive('dual')"
                  :class="{ active: isStreamModeActive('dual') }"
                  class="mode-card"
                  :disabled="pending"
                  type="button"
                  @click="startDraft('dual')"
                >
                  <span class="mode-card__check" aria-hidden="true">
                    <Icon
                      v-if="isStreamModeActive('dual')"
                      class="mode-card__check-icon"
                      icon="ph:check-bold"
                      width="15"
                    />
                  </span>
                  <span class="mode-card__title">{{ t("modes.dual") }}</span>
                  <span class="mode-card__body">{{
                    t("wizard.modeDualBody")
                  }}</span>
                </button>
              </div>

              <div
                v-else-if="currentStep === 1"
                class="wizard-form wizard-form--split"
              >
                <FieldBlock
                  :hint="t('wizard.nameHint')"
                  :label="t('wizard.nameLabel')"
                >
                  <input v-model="draft.name" type="text" />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.logRootHint')"
                  :label="t('wizard.logRootLabel')"
                >
                  <input
                    v-model="draft.payload.shared.log.root_path"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.logLevelHint')"
                  :label="t('wizard.logLevelLabel')"
                >
                  <select v-model="draft.payload.shared.log.level">
                    <option
                      v-for="option in logLevelOptions"
                      :key="option.value"
                      :value="option.value"
                    >
                      {{ option.label }}
                    </option>
                  </select>
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.logPrefixHint')"
                  :label="t('wizard.logPrefixLabel')"
                >
                  <input
                    v-model="draft.payload.shared.log.prefix"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.memoryModeHint')"
                  :label="t('wizard.memoryModeLabel')"
                >
                  <select v-model="draft.payload.shared.general.memory_mode">
                    <option
                      v-for="option in memoryOptions"
                      :key="option.value"
                      :value="option.value"
                    >
                      {{ option.label }}
                    </option>
                  </select>
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.encipherKeyHint')"
                  :label="t('wizard.encipherKeyLabel')"
                >
                  <SensitiveInput
                    v-model="draft.payload.shared.general.encipher_key"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.encipherIvHint')"
                  :label="t('wizard.encipherIvLabel')"
                >
                  <SensitiveInput
                    v-model="draft.payload.shared.general.encipher_iv"
                  />
                </FieldBlock>
              </div>

              <div
                v-else-if="currentStep === 2"
                class="wizard-form wizard-form--split"
              >
                <FieldBlock
                  :hint="t('wizard.embyUrlHint')"
                  :label="t('wizard.embyUrlLabel')"
                >
                  <input v-model="draft.payload.shared.emby.url" type="text" />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.embyPortHint')"
                  :label="t('wizard.embyPortLabel')"
                >
                  <input v-model="draft.payload.shared.emby.port" type="text" />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.embyTokenHint')"
                  :label="t('wizard.embyTokenLabel')"
                >
                  <SensitiveInput v-model="draft.payload.shared.emby.token" />
                </FieldBlock>

                <FieldBlock
                  v-if="showFrontend && draft.payload.frontend"
                  :hint="t('wizard.frontendPortHint')"
                  :label="t('wizard.frontendPortLabel')"
                >
                  <input
                    v-model.number="draft.payload.frontend.listen_port"
                    type="number"
                  />
                </FieldBlock>

                <FieldBlock
                  v-if="showBackend && draft.payload.backend"
                  :hint="t('wizard.backendPortHint')"
                  :label="t('wizard.backendPortLabel')"
                >
                  <input
                    v-model.number="draft.payload.backend.listen_port"
                    type="number"
                  />
                </FieldBlock>

                <FieldBlock
                  v-if="showBackend && draft.payload.backend"
                  :hint="t('wizard.backendBaseUrlHint')"
                  :label="t('wizard.backendBaseUrlLabel')"
                >
                  <input v-model="draft.payload.backend.base_url" type="text" />
                </FieldBlock>

                <FieldBlock
                  v-if="showBackend && draft.payload.backend"
                  :hint="t('wizard.backendPublicPortHint')"
                  :label="t('wizard.backendPublicPortLabel')"
                >
                  <input v-model="draft.payload.backend.port" type="text" />
                </FieldBlock>

                <FieldBlock
                  v-if="showBackend && draft.payload.backend"
                  :hint="t('wizard.backendPathHint')"
                  :label="t('wizard.backendPathLabel')"
                >
                  <input v-model="draft.payload.backend.path" type="text" />
                </FieldBlock>

                <FieldBlock
                  v-if="showBackend && draft.payload.backend"
                  class="field--span-two"
                  :hint="t('wizard.problematicClientsHint')"
                  :label="t('wizard.problematicClientsLabel')"
                >
                  <textarea
                    v-model="problematicClientsText"
                    rows="5"
                  ></textarea>
                </FieldBlock>
              </div>

              <div
                v-else-if="currentStep === 3"
                class="wizard-form wizard-form--split"
              >
                <FieldBlock
                  :hint="t('wizard.userAgentModeHint')"
                  :label="t('wizard.userAgentModeLabel')"
                >
                  <select v-model="draft.payload.shared.user_agent.mode">
                    <option
                      v-for="option in userAgentModeOptions"
                      :key="option.value"
                      :value="option.value"
                    >
                      {{ option.label }}
                    </option>
                  </select>
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.fallbackPathHint')"
                  :label="t('wizard.fallbackPathLabel')"
                >
                  <input
                    v-model="draft.payload.shared.fallback.video_missing_path"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  class="field--span-two"
                  :hint="t('wizard.userAgentRulesHint')"
                  :label="t('wizard.userAgentRulesLabel')"
                >
                  <textarea v-model="userAgentRulesText" rows="6"></textarea>
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.sslCertHint')"
                  :label="t('wizard.sslCertLabel')"
                >
                  <input
                    v-model="draft.payload.shared.http2.ssl_cert_file"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.sslKeyHint')"
                  :label="t('wizard.sslKeyLabel')"
                >
                  <input
                    v-model="draft.payload.shared.http2.ssl_key_file"
                    type="text"
                  />
                </FieldBlock>

                <template v-if="showFrontend && draft.payload.frontend">
                  <FieldBlock
                    :hint="t('wizard.frontendAntiReverseHint')"
                    :label="t('wizard.frontendAntiReverseLabel')"
                  >
                    <label class="toggle-row">
                      <input
                        v-model="
                          draft.payload.frontend.anti_reverse_proxy.enable
                        "
                        type="checkbox"
                      />
                      <span>{{ t("wizard.enableProtection") }}</span>
                    </label>
                  </FieldBlock>

                  <FieldBlock
                    :hint="t('wizard.frontendAntiHostHint')"
                    :label="t('wizard.frontendAntiHostLabel')"
                  >
                    <input
                      :value="
                        hostsToText(
                          draft.payload.frontend.anti_reverse_proxy.host,
                        )
                      "
                      type="text"
                      @change="
                        draft.payload.frontend.anti_reverse_proxy.host =
                          textToHosts(($event.target as HTMLInputElement).value)
                      "
                    />
                  </FieldBlock>
                </template>

                <div class="field--span-two wizard-inline-section">
                  <p class="section-label">
                    {{ t("wizard.deployNginxTitle") }}
                  </p>
                  <p class="lede">{{ t("wizard.deployNginxBody") }}</p>
                </div>

                <FieldBlock
                  v-if="showFrontend"
                  :hint="t('wizard.nginxFrontendServerNameHint')"
                  :label="t('wizard.nginxFrontendServerNameLabel')"
                >
                  <input
                    v-model="draft.payload.nginx.frontend.server_name"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  v-if="showFrontend"
                  :hint="t('wizard.nginxFrontendSslCertHint')"
                  :label="t('wizard.nginxFrontendSslCertLabel')"
                >
                  <input
                    v-model="draft.payload.nginx.frontend.ssl_certificate"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  v-if="showFrontend"
                  :hint="t('wizard.nginxFrontendSslKeyHint')"
                  :label="t('wizard.nginxFrontendSslKeyLabel')"
                >
                  <input
                    v-model="draft.payload.nginx.frontend.ssl_certificate_key"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  v-if="showBackend"
                  :hint="t('wizard.nginxBackendServerNameHint')"
                  :label="t('wizard.nginxBackendServerNameLabel')"
                >
                  <input
                    v-model="draft.payload.nginx.backend.server_name"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  v-if="showBackend"
                  :hint="t('wizard.nginxBackendSslCertHint')"
                  :label="t('wizard.nginxBackendSslCertLabel')"
                >
                  <input
                    v-model="draft.payload.nginx.backend.ssl_certificate"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  v-if="showBackend"
                  :hint="t('wizard.nginxBackendSslKeyHint')"
                  :label="t('wizard.nginxBackendSslKeyLabel')"
                >
                  <input
                    v-model="draft.payload.nginx.backend.ssl_certificate_key"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  v-if="showBackend"
                  :hint="t('wizard.nginxResolverProviderHint')"
                  :label="t('wizard.nginxResolverProviderLabel')"
                >
                  <select
                    v-model="draft.payload.nginx.backend.resolver_provider"
                  >
                    <option
                      v-for="option in resolverProviderOptions"
                      :key="option.value"
                      :value="option.value"
                    >
                      {{ option.label }}
                    </option>
                  </select>
                </FieldBlock>

                <FieldBlock
                  v-if="
                    showBackend &&
                    draft.payload.nginx.backend.resolver_provider === 'custom'
                  "
                  :hint="t('wizard.nginxCustomResolversHint')"
                  :label="t('wizard.nginxCustomResolversLabel')"
                >
                  <input
                    v-model="draft.payload.nginx.backend.custom_resolvers"
                    type="text"
                  />
                </FieldBlock>

                <div class="field--span-two wizard-inline-section">
                  <p class="section-label">
                    {{ t("wizard.deployRuntimeTitle") }}
                  </p>
                  <p class="lede">{{ t("wizard.deployRuntimeBody") }}</p>
                </div>

                <FieldBlock
                  :hint="t('wizard.systemdBinaryPathHint')"
                  :label="t('wizard.systemdBinaryPathLabel')"
                >
                  <input
                    v-model="draft.payload.deployment.systemd.binary_path"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.systemdWorkingDirectoryHint')"
                  :label="t('wizard.systemdWorkingDirectoryLabel')"
                >
                  <input
                    v-model="draft.payload.deployment.systemd.working_directory"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.systemdConfigPathHint')"
                  :label="t('wizard.systemdConfigPathLabel')"
                >
                  <input
                    v-model="draft.payload.deployment.systemd.config_path"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.pm2BinaryPathHint')"
                  :label="t('wizard.pm2BinaryPathLabel')"
                >
                  <input
                    v-model="draft.payload.deployment.pm2.binary_path"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.pm2WorkingDirectoryHint')"
                  :label="t('wizard.pm2WorkingDirectoryLabel')"
                >
                  <input
                    v-model="draft.payload.deployment.pm2.working_directory"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.pm2ConfigPathHint')"
                  :label="t('wizard.pm2ConfigPathLabel')"
                >
                  <input
                    v-model="draft.payload.deployment.pm2.config_path"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.pm2OutFileHint')"
                  :label="t('wizard.pm2OutFileLabel')"
                >
                  <input
                    v-model="draft.payload.deployment.pm2.out_file"
                    type="text"
                  />
                </FieldBlock>

                <FieldBlock
                  :hint="t('wizard.pm2ErrorFileHint')"
                  :label="t('wizard.pm2ErrorFileLabel')"
                >
                  <input
                    v-model="draft.payload.deployment.pm2.error_file"
                    type="text"
                  />
                </FieldBlock>
              </div>

              <div v-else-if="currentStep === 4" class="rewrite-lab">
                <div
                  v-if="showFrontend && draft.payload.frontend"
                  class="rewrite-lab__panel"
                >
                  <div class="rewrite-lab__intro">
                    <div>
                      <p class="section-label">
                        {{ t("wizard.frontendRewriteTitle") }}
                      </p>
                      <p class="lede">{{ t("wizard.frontendRewriteBody") }}</p>
                    </div>
                    <button
                      class="ghost-button"
                      type="button"
                      @click="addFrontendRewrite"
                    >
                      {{ t("wizard.addRewrite") }}
                    </button>
                  </div>

                  <div
                    v-if="draft.payload.frontend.path_rewrites.length"
                    class="rewrite-list"
                  >
                    <article
                      v-for="(rewrite, index) in draft.payload.frontend
                        .path_rewrites"
                      :key="`frontend-rewrite-${index}`"
                      class="rewrite-card"
                    >
                      <div class="rewrite-card__head">
                        <label class="toggle-row">
                          <input v-model="rewrite.enable" type="checkbox" />
                          <span>{{ t("wizard.rewriteEnabled") }}</span>
                        </label>
                        <button
                          class="ghost-button ghost-button--danger"
                          type="button"
                          @click="removeFrontendRewrite(index)"
                        >
                          {{ t("common.delete") }}
                        </button>
                      </div>

                      <FieldBlock
                        :hint="t('wizard.rewritePatternHint')"
                        :label="t('wizard.rewritePatternLabel')"
                      >
                        <input v-model="rewrite.pattern" type="text" />
                      </FieldBlock>

                      <FieldBlock
                        :hint="t('wizard.rewriteReplacementHint')"
                        :label="t('wizard.rewriteReplacementLabel')"
                      >
                        <input v-model="rewrite.replacement" type="text" />
                      </FieldBlock>

                      <p
                        v-if="frontendRewriteRulePreview(rewrite).error"
                        class="rewrite-card__status rewrite-card__status--error"
                      >
                        {{ t("wizard.rewriteRegexInvalid") }}
                        {{ frontendRewriteRulePreview(rewrite).error }}
                      </p>
                      <p
                        v-else-if="frontendRewriteRulePreview(rewrite).matched"
                        class="rewrite-card__status"
                      >
                        {{
                          t("wizard.rewriteRegexMatched", {
                            output: frontendRewriteRulePreview(rewrite).output,
                          })
                        }}
                      </p>
                      <p
                        v-else
                        class="rewrite-card__status rewrite-card__status--muted"
                      >
                        {{ t("wizard.rewriteRegexNoMatch") }}
                      </p>
                    </article>
                  </div>
                  <div v-else class="panel-empty">
                    {{ t("wizard.noRewriteRules") }}
                  </div>

                  <div class="test-card">
                    <FieldBlock
                      :hint="t('wizard.routeTestHint')"
                      :label="t('wizard.routeTestInputLabel')"
                    >
                      <input v-model="routeTestInput" type="text" />
                    </FieldBlock>
                    <div class="test-card__result">
                      <p>{{ t("wizard.routeTestResultLabel") }}</p>
                      <code>{{
                        frontendRewritePreview.output || routeTestInput
                      }}</code>
                      <p
                        v-if="frontendRewritePreview.error"
                        class="test-card__error"
                      >
                        {{ frontendRewritePreview.error }}
                      </p>
                      <ul
                        v-else-if="frontendRewritePreview.applied.length"
                        class="test-card__applied"
                      >
                        <li
                          v-for="item in frontendRewritePreview.applied"
                          :key="item"
                        >
                          {{ item }}
                        </li>
                      </ul>
                      <p v-else class="test-card__muted">
                        {{ t("wizard.routeTestNoMatch") }}
                      </p>
                    </div>
                  </div>
                </div>

                <div class="rewrite-lab__panel">
                  <p class="section-label">
                    {{ t("wizard.routeChecklistTitle") }}
                  </p>
                  <ul class="checklist">
                    <li>{{ t("wizard.routeChecklistItem1") }}</li>
                    <li>{{ t("wizard.routeChecklistItem2") }}</li>
                    <li>{{ t("wizard.routeChecklistItem3") }}</li>
                  </ul>
                </div>
              </div>

              <div v-else-if="currentStep === 5">
                <div v-if="showBackend" class="node-stage">
                  <div class="node-stage__intro">
                    <div>
                      <p class="section-label">
                        {{ t("wizard.nodeFlowTitle") }}
                      </p>
                      <p class="lede">{{ t("wizard.nodeFlowBody") }}</p>
                    </div>
                  </div>
                  <div class="node-toolbar">
                    <button
                      v-for="option in backendNodeTypeOptions"
                      :key="option.value"
                      class="ghost-button"
                      type="button"
                      @click="addBackendNode(option.value)"
                    >
                      {{ t("wizard.addNode") }} {{ option.label }}
                    </button>
                  </div>

                  <div class="node-stack">
                    <article
                      v-for="(node, index) in draft.payload.backend_nodes"
                      :key="`${node.name}-${index}`"
                      class="node-card"
                    >
                      <div class="node-card__header">
                        <div>
                          <p class="node-card__eyebrow">
                            {{ t("wizard.nodeLabel") }} {{ index + 1 }}
                          </p>
                          <h3>
                            {{
                              node.name ||
                              `${t("wizard.nodeLabel")} ${index + 1}`
                            }}
                          </h3>
                        </div>
                        <div class="node-card__actions">
                          <select
                            class="node-card__type"
                            :disabled="true"
                            :value="node.backend_type"
                          >
                            <option
                              v-for="option in backendNodeTypeOptions"
                              :key="option.value"
                              :value="option.value"
                            >
                              {{ option.label }}
                            </option>
                          </select>
                          <button
                            class="ghost-button ghost-button--danger"
                            type="button"
                            @click="removeBackendNode(index)"
                          >
                            {{ t("common.delete") }}
                          </button>
                        </div>
                      </div>

                      <div class="wizard-form wizard-form--split">
                        <FieldBlock
                          :hint="t('wizard.nodeNameHint')"
                          :label="t('wizard.nodeNameLabel')"
                        >
                          <input v-model="node.name" type="text" />
                        </FieldBlock>

                        <FieldBlock
                          :hint="t('wizard.nodePatternHint')"
                          :label="t('wizard.nodePatternLabel')"
                        >
                          <input v-model="node.pattern" type="text" />
                        </FieldBlock>

                        <FieldBlock
                          :hint="t('wizard.nodeBaseUrlHint')"
                          :label="t('wizard.nodeBaseUrlLabel')"
                        >
                          <input v-model="node.base_url" type="text" />
                        </FieldBlock>

                        <FieldBlock
                          :hint="t('wizard.nodePortHint')"
                          :label="t('wizard.nodePortLabel')"
                        >
                          <input v-model="node.port" type="text" />
                        </FieldBlock>

                        <FieldBlock
                          :hint="t('wizard.nodePathHint')"
                          :label="t('wizard.nodePathLabel')"
                        >
                          <input v-model="node.path" type="text" />
                        </FieldBlock>

                        <FieldBlock
                          :hint="t('wizard.nodePriorityHint')"
                          :label="t('wizard.nodePriorityLabel')"
                        >
                          <input v-model.number="node.priority" type="number" />
                        </FieldBlock>

                        <FieldBlock
                          :hint="t('wizard.nodeProxyModeHint')"
                          :label="t('wizard.nodeProxyModeLabel')"
                        >
                          <select v-model="node.proxy_mode">
                            <option
                              v-for="option in proxyModeOptions"
                              :key="option.value"
                              :value="option.value"
                            >
                              {{ option.label }}
                            </option>
                          </select>
                        </FieldBlock>

                        <FieldBlock
                          :hint="t('wizard.nodeSpeedLimitHint')"
                          :label="t('wizard.nodeSpeedLimitLabel')"
                        >
                          <input
                            v-model.number="node.client_speed_limit_kbs"
                            type="number"
                          />
                        </FieldBlock>

                        <FieldBlock
                          :hint="t('wizard.nodeBurstLimitHint')"
                          :label="t('wizard.nodeBurstLimitLabel')"
                        >
                          <input
                            v-model.number="node.client_burst_speed_kbs"
                            type="number"
                          />
                        </FieldBlock>

                        <FieldBlock
                          :hint="t('wizard.nodeAntiReverseHint')"
                          :label="t('wizard.nodeAntiReverseLabel')"
                        >
                          <label class="toggle-row">
                            <input
                              v-model="node.anti_reverse_proxy.enable"
                              type="checkbox"
                            />
                            <span>{{ t("wizard.enableProtection") }}</span>
                          </label>
                        </FieldBlock>

                        <FieldBlock
                          :hint="t('wizard.nodeAntiHostHint')"
                          :label="t('wizard.nodeAntiHostLabel')"
                        >
                          <input
                            :value="hostsToText(node.anti_reverse_proxy.host)"
                            type="text"
                            @change="
                              node.anti_reverse_proxy.host = textToHosts(
                                ($event.target as HTMLInputElement).value,
                              )
                            "
                          />
                        </FieldBlock>
                      </div>

                      <div class="node-section">
                        <div class="node-section__head">
                          <p class="section-label">
                            {{ t("wizard.nodeRewriteTitle") }}
                          </p>
                          <button
                            class="ghost-button"
                            type="button"
                            @click="addNodeRewrite(index)"
                          >
                            {{ t("wizard.addRewrite") }}
                          </button>
                        </div>

                        <div
                          v-if="node.path_rewrites.length"
                          class="rewrite-list"
                        >
                          <article
                            v-for="(
                              rewrite, rewriteIndex
                            ) in node.path_rewrites"
                            :key="`${node.name}-rewrite-${rewriteIndex}`"
                            class="rewrite-card"
                          >
                            <div class="rewrite-card__head">
                              <label class="toggle-row">
                                <input
                                  v-model="rewrite.enable"
                                  type="checkbox"
                                />
                                <span>{{ t("wizard.rewriteEnabled") }}</span>
                              </label>
                              <button
                                class="ghost-button ghost-button--danger"
                                type="button"
                                @click="removeNodeRewrite(index, rewriteIndex)"
                              >
                                {{ t("common.delete") }}
                              </button>
                            </div>
                            <FieldBlock
                              :hint="t('wizard.rewritePatternHint')"
                              :label="t('wizard.rewritePatternLabel')"
                            >
                              <input v-model="rewrite.pattern" type="text" />
                            </FieldBlock>
                            <FieldBlock
                              :hint="t('wizard.rewriteReplacementHint')"
                              :label="t('wizard.rewriteReplacementLabel')"
                            >
                              <input
                                v-model="rewrite.replacement"
                                type="text"
                              />
                            </FieldBlock>

                            <p
                              v-if="
                                backendNodeRewriteRulePreview(
                                  index,
                                  node,
                                  rewrite,
                                ).error
                              "
                              class="rewrite-card__status rewrite-card__status--error"
                            >
                              {{ t("wizard.rewriteRegexInvalid") }}
                              {{
                                backendNodeRewriteRulePreview(
                                  index,
                                  node,
                                  rewrite,
                                ).error
                              }}
                            </p>
                            <p
                              v-else-if="
                                backendNodeRewriteRulePreview(
                                  index,
                                  node,
                                  rewrite,
                                ).matched
                              "
                              class="rewrite-card__status"
                            >
                              {{
                                t("wizard.rewriteRegexMatched", {
                                  output: backendNodeRewriteRulePreview(
                                    index,
                                    node,
                                    rewrite,
                                  ).output,
                                })
                              }}
                            </p>
                            <p
                              v-else
                              class="rewrite-card__status rewrite-card__status--muted"
                            >
                              {{ t("wizard.rewriteRegexNoMatch") }}
                            </p>
                          </article>
                        </div>
                        <div v-else class="panel-empty">
                          {{ t("wizard.noRewriteRules") }}
                        </div>

                        <div class="test-card">
                          <FieldBlock
                            :hint="t('wizard.nodeRouteTestHint')"
                            :label="t('wizard.routeTestInputLabel')"
                          >
                            <input
                              v-model="nodeRewriteSamples[index]"
                              :placeholder="
                                node.pattern || DEFAULT_BACKEND_TEST_PATH
                              "
                              type="text"
                            />
                          </FieldBlock>
                          <div class="test-card__result">
                            <p>{{ t("wizard.routeTestResultLabel") }}</p>
                            <code>{{
                              backendNodeRewritePreview(index, node).output ||
                              getNodeRewriteSample(index, node)
                            }}</code>
                            <p
                              v-if="
                                backendNodeRewritePreview(index, node).error
                              "
                              class="test-card__error"
                            >
                              {{ backendNodeRewritePreview(index, node).error }}
                            </p>
                            <ul
                              v-else-if="
                                backendNodeRewritePreview(index, node).applied
                                  .length
                              "
                              class="test-card__applied"
                            >
                              <li
                                v-for="item in backendNodeRewritePreview(
                                  index,
                                  node,
                                ).applied"
                                :key="item"
                              >
                                {{ item }}
                              </li>
                            </ul>
                            <p v-else class="test-card__muted">
                              {{ t("wizard.routeTestNoMatch") }}
                            </p>
                          </div>
                        </div>
                      </div>

                      <div class="node-section">
                        <p class="section-label">
                          {{ t("wizard.nodeSettingsTitle") }}
                        </p>

                        <div
                          v-if="node.backend_type === 'Disk' && node.disk"
                          class="wizard-form wizard-form--split"
                        >
                          <FieldBlock
                            :hint="t('wizard.diskDescriptionHint')"
                            :label="t('wizard.diskDescriptionLabel')"
                          >
                            <input
                              v-model="node.disk.description"
                              type="text"
                            />
                          </FieldBlock>
                        </div>

                        <div
                          v-else-if="
                            node.backend_type === 'OpenList' && node.open_list
                          "
                          class="wizard-form wizard-form--split"
                        >
                          <FieldBlock
                            :hint="t('wizard.openListBaseUrlHint')"
                            :label="t('wizard.openListBaseUrlLabel')"
                          >
                            <input
                              v-model="node.open_list.base_url"
                              type="text"
                            />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.openListPortHint')"
                            :label="t('wizard.openListPortLabel')"
                          >
                            <input v-model="node.open_list.port" type="text" />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.openListTokenHint')"
                            :label="t('wizard.openListTokenLabel')"
                          >
                            <SensitiveInput v-model="node.open_list.token" />
                          </FieldBlock>
                        </div>

                        <div
                          v-else-if="
                            node.backend_type === 'DirectLink' &&
                            node.direct_link
                          "
                          class="wizard-form wizard-form--split"
                        >
                          <FieldBlock
                            :hint="t('wizard.directLinkUserAgentHint')"
                            :label="t('wizard.directLinkUserAgentLabel')"
                          >
                            <input
                              v-model="node.direct_link.user_agent"
                              type="text"
                            />
                          </FieldBlock>
                        </div>

                        <div
                          v-else-if="
                            node.backend_type === 'googleDrive' &&
                            node.google_drive
                          "
                          class="wizard-form wizard-form--split"
                        >
                          <FieldBlock
                            :hint="t('wizard.googleNodeUuidHint')"
                            :label="t('wizard.googleNodeUuidLabel')"
                          >
                            <input
                              v-model="node.google_drive.node_uuid"
                              type="text"
                            />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.googleClientIdHint')"
                            :label="t('wizard.googleClientIdLabel')"
                          >
                            <input
                              v-model="node.google_drive.client_id"
                              type="text"
                            />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.googleClientSecretHint')"
                            :label="t('wizard.googleClientSecretLabel')"
                          >
                            <SensitiveInput
                              v-model="node.google_drive.client_secret"
                            />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.googleDriveIdHint')"
                            :label="t('wizard.googleDriveIdLabel')"
                          >
                            <input
                              v-model="node.google_drive.drive_id"
                              type="text"
                            />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.googleDriveNameHint')"
                            :label="t('wizard.googleDriveNameLabel')"
                          >
                            <input
                              v-model="node.google_drive.drive_name"
                              type="text"
                            />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.googleAccessTokenHint')"
                            :label="t('wizard.googleAccessTokenLabel')"
                          >
                            <SensitiveInput
                              v-model="node.google_drive.access_token"
                            />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.googleRefreshTokenHint')"
                            :label="t('wizard.googleRefreshTokenLabel')"
                          >
                            <SensitiveInput
                              v-model="node.google_drive.refresh_token"
                            />
                          </FieldBlock>
                        </div>

                        <div
                          v-else-if="
                            node.backend_type === 'WebDav' && node.webdav
                          "
                          class="wizard-form wizard-form--split"
                        >
                          <FieldBlock
                            :hint="t('wizard.webdavNodeUuidHint')"
                            :label="t('wizard.webdavNodeUuidLabel')"
                          >
                            <input
                              v-model="node.webdav.node_uuid"
                              type="text"
                            />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.webdavUrlModeHint')"
                            :label="t('wizard.webdavUrlModeLabel')"
                          >
                            <select v-model="node.webdav.url_mode">
                              <option
                                v-for="option in urlModeOptions"
                                :key="option.value"
                                :value="option.value"
                              >
                                {{ option.label }}
                              </option>
                            </select>
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.webdavQueryParamHint')"
                            :label="t('wizard.webdavQueryParamLabel')"
                          >
                            <input
                              v-model="node.webdav.query_param"
                              type="text"
                            />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.webdavUrlTemplateHint')"
                            :label="t('wizard.webdavUrlTemplateLabel')"
                          >
                            <input
                              v-model="node.webdav.url_template"
                              type="text"
                            />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.webdavUsernameHint')"
                            :label="t('wizard.webdavUsernameLabel')"
                          >
                            <input v-model="node.webdav.username" type="text" />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.webdavPasswordHint')"
                            :label="t('wizard.webdavPasswordLabel')"
                          >
                            <SensitiveInput v-model="node.webdav.password" />
                          </FieldBlock>
                          <FieldBlock
                            :hint="t('wizard.webdavUserAgentHint')"
                            :label="t('wizard.webdavUserAgentLabel')"
                          >
                            <input
                              v-model="node.webdav.user_agent"
                              type="text"
                            />
                          </FieldBlock>
                        </div>
                      </div>
                    </article>
                  </div>
                </div>

                <div v-else class="panel-empty panel-empty--wide">
                  <p class="section-label">{{ t("wizard.nodeFlowTitle") }}</p>
                  <p class="lede">{{ t("wizard.noBackendNodesNeeded") }}</p>
                </div>
              </div>

              <div v-else-if="isReviewStep" class="review-panel">
                <p class="section-label">{{ t("wizard.reviewTitle") }}</p>
                <p class="lede">{{ t("wizard.reviewBody") }}</p>
                <dl class="review-panel__list">
                  <div>
                    <dt>{{ t("wizard.reviewName") }}</dt>
                    <dd>{{ draft.name }}</dd>
                  </div>
                  <div>
                    <dt>{{ t("wizard.reviewMode") }}</dt>
                    <dd>{{ t(`modes.${draft.stream_mode}`) }}</dd>
                  </div>
                  <div v-if="draft.payload.frontend">
                    <dt>{{ t("wizard.reviewFrontendPort") }}</dt>
                    <dd>{{ draft.payload.frontend.listen_port }}</dd>
                  </div>
                  <div v-if="draft.payload.backend">
                    <dt>{{ t("wizard.reviewBackendPort") }}</dt>
                    <dd>{{ draft.payload.backend.listen_port }}</dd>
                  </div>
                  <div>
                    <dt>{{ t("wizard.reviewNodes") }}</dt>
                    <dd>{{ draft.payload.backend_nodes.length }}</dd>
                  </div>
                  <div>
                    <dt>{{ t("wizard.reviewRewrites") }}</dt>
                    <dd>
                      {{
                        enabledRewriteCount(
                          draft.payload.frontend?.path_rewrites ?? [],
                        )
                      }}
                      /
                      {{
                        draft.payload.backend_nodes.reduce(
                          (count, node) =>
                            count + enabledRewriteCount(node.path_rewrites),
                          0,
                        )
                      }}
                    </dd>
                  </div>
                  <div>
                    <dt>{{ t("wizard.reviewArtifacts") }}</dt>
                    <dd>
                      config.toml · nginx.conf · docker-compose.yaml ·
                      systemd.service · pm2.config.cjs
                    </dd>
                  </div>
                </dl>

                <div class="rewrite-lab__panel">
                  <p class="section-label">
                    {{ t("wizard.reviewChecklistTitle") }}
                  </p>
                  <ul class="checklist">
                    <li>{{ t("wizard.reviewChecklistItem1") }}</li>
                    <li>{{ t("wizard.reviewChecklistItem2") }}</li>
                    <li>{{ t("wizard.reviewChecklistItem3") }}</li>
                    <li>{{ t("wizard.reviewChecklistItem4") }}</li>
                  </ul>
                </div>
              </div>
            </div>
          </Transition>

          <div class="wizard-actions">
            <button
              :disabled="currentStep === 0"
              type="button"
              @click="previousStep"
            >
              {{ t("common.previous") }}
            </button>
            <button
              :class="{
                'wizard-actions__primary': currentStep !== finalStepIndex,
              }"
              :disabled="currentStep === finalStepIndex"
              type="button"
              @click="nextStep"
            >
              {{ t("common.next") }}
            </button>
            <button type="button" @click="persistDraft(false)">
              {{ saving ? t("wizard.saving") : t("wizard.saveDraft") }}
            </button>
            <button
              v-if="isReviewStep"
              class="wizard-actions__primary"
              :disabled="pending"
              type="button"
              @click="generate"
            >
              {{ pending ? t("wizard.generating") : t("wizard.generateFiles") }}
            </button>
          </div>

          <div v-if="artifacts.length" class="preview-stack">
            <h2>{{ t("wizard.previewTitle") }}</h2>
            <p class="lede">{{ t("wizard.previewBody") }}</p>
            <FilePreviewPanel
              v-for="artifact in artifacts"
              :key="artifact.file_name"
              :collapse-label="t('common.collapse')"
              :collapsed="collapsedArtifacts[artifact.file_name]"
              :content="artifact.content"
              :download-href="
                latestConfigSetId
                  ? CONFIG_SETS_API.artifactDownload(
                      latestConfigSetId,
                      artifact.artifact_type,
                    )
                  : undefined
              "
              :download-label="t('common.download')"
              :expand-label="t('common.expand')"
              :file-name="artifact.file_name"
              :language="artifact.language"
              @toggle="toggleArtifact(artifact.file_name)"
            />
          </div>
        </template>
      </GlassPanel>
    </section>
  </AppWorkspaceShell>
</template>

<style scoped>
.wizard-stage__status {
  display: grid;
  gap: 0.45rem;
  width: 100%;
  padding: 1rem 1.1rem;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  background: var(--bg-surface);
  box-shadow: var(--shadow-soft);
}

.wizard-stage__note {
  margin: 0;
  color: var(--text-muted);
  font-size: 0.9rem;
  line-height: 1.5;
}

.wizard-stage__note--error {
  color: var(--signal-red);
}

.wizard-stage__note--info {
  color: var(--text-main);
}

.wizard-stage__grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  min-width: 0;
  width: 100%;
  position: relative;
}

.wizard-toast {
  position: sticky;
  top: 1rem;
  z-index: 15;
  display: inline-flex;
  align-items: center;
  gap: 0.65rem;
  width: fit-content;
  max-width: min(30rem, 100%);
  margin-left: auto;
  padding: 0.9rem 1rem;
  border-color: color-mix(
    in srgb,
    var(--signal-warm) 22%,
    var(--border-subtle)
  );
  box-shadow: var(--shadow-medium);
}

.wizard-toast p {
  margin: 0;
  color: var(--text-main);
  line-height: 1.45;
}

.toast-pop-enter-active,
.toast-pop-leave-active {
  transition:
    opacity 220ms var(--curve-swift),
    transform 260ms var(--curve-spring);
}

.toast-pop-enter-from,
.toast-pop-leave-to {
  opacity: 0;
  transform: translateY(-10px);
}

.wizard-stage__editor {
  display: grid;
  gap: 1.65rem;
  width: 100%;
  padding: 1.5rem 0 2rem;
  background: transparent;
  border: 0;
  box-shadow: none;
  min-width: 0;
}

.wizard-stage__icon {
  width: 3.4rem;
  height: 3.4rem;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 1rem;
  background: color-mix(in srgb, var(--signal-warm) 10%, var(--bg-surface));
  color: var(--signal-warm);
  border: 1px solid
    color-mix(in srgb, var(--signal-warm) 12%, var(--border-subtle));
}

.wizard-stage__line {
  width: 4.2rem;
  height: 0.28rem;
  border-radius: 999px;
  background: var(--border-subtle);
}

.mode-grid,
.choice-list {
  display: grid;
  gap: 1rem;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  min-width: 0;
}

.mode-card {
  display: grid;
  grid-template-columns: 2.5rem 1fr;
  gap: 0.8rem;
  align-content: start;
  min-height: 9.5rem;
  padding: 1.15rem 1.2rem;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  background: var(--bg-surface);
  box-shadow: var(--shadow-soft);
  color: var(--text-main);
  text-align: left;
  width: 100%;
  cursor: pointer;
  transition:
    background-color 180ms var(--curve-swift),
    border-color 180ms var(--curve-swift),
    transform 180ms var(--curve-swift),
    box-shadow 180ms var(--curve-swift);
}

.mode-card:hover {
  background: var(--bg-surface-strong);
  border-color: var(--border-strong);
}

.mode-card.active {
  background: color-mix(
    in srgb,
    var(--bg-surface-strong) 88%,
    var(--bg-accent)
  );
  border-color: var(--border-accent);
  box-shadow: 0 0 0 1px
    color-mix(in srgb, var(--brand-secondary) 14%, transparent);
}

.mode-card__check {
  grid-column: 1;
  grid-row: 1 / span 2;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 2.15rem;
  height: 2.15rem;
  border-radius: 0.85rem;
  border: 1px solid var(--border-subtle);
  background: var(--bg-accent);
  color: transparent;
}

.mode-card.active .mode-card__check {
  border-color: color-mix(
    in srgb,
    var(--signal-blue) 34%,
    var(--border-subtle)
  );
  background: color-mix(in srgb, var(--signal-blue) 18%, var(--bg-surface));
  color: var(--signal-blue);
}

.mode-card__title {
  grid-column: 2;
  font-family: var(--display-font);
  font-size: 1rem;
  font-weight: 700;
}

.mode-card__body {
  grid-column: 2;
  color: var(--text-muted);
  font-size: 0.9375rem;
  line-height: 1.5;
}

.wizard-form {
  display: grid;
  gap: 1rem;
  width: 100%;
}

.wizard-form--split {
  grid-template-columns: repeat(2, minmax(0, 1fr));
  align-items: start;
}

.wizard-form :deep(.field) {
  width: 100%;
}

.field--span-two {
  grid-column: 1 / -1;
}

.wizard-inline-section {
  display: grid;
  gap: 0.35rem;
}

.wizard-inline-section .lede,
.wizard-inline-section .section-label {
  margin: 0;
}

.wizard-form input,
.wizard-form select,
.wizard-form textarea {
  width: 100%;
  min-height: 2.95rem;
}

.wizard-form textarea {
  resize: vertical;
  min-height: 7rem;
}

.toggle-row {
  display: inline-flex;
  align-items: center;
  gap: 0.7rem;
  min-height: 2.75rem;
  color: var(--text-main);
}

.toggle-row input {
  width: 1rem;
  min-height: auto;
}

.rewrite-lab,
.node-stage {
  display: grid;
  gap: 1rem;
}

.rewrite-lab__panel,
.node-card,
.review-panel,
.test-card {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  background: var(--bg-surface);
  box-shadow: var(--shadow-soft);
}

.rewrite-lab__panel,
.review-panel {
  display: grid;
  gap: 1.15rem;
  padding: 1.15rem 1.15rem 1.2rem;
}

.rewrite-lab__intro,
.node-stage__intro,
.node-card__header,
.node-section__head,
.rewrite-card__head {
  display: flex;
  justify-content: space-between;
  gap: 1rem;
  align-items: center;
}

.rewrite-list,
.node-stack {
  display: grid;
  gap: 0.9rem;
}

.rewrite-card {
  display: grid;
  gap: 0.9rem;
  padding: 0.9rem;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  background: var(--bg-soft);
}

.rewrite-card__status {
  margin: 0;
  color: var(--signal-blue);
  font-size: 0.875rem;
}

.rewrite-card__status--error {
  color: var(--signal-red);
}

.rewrite-card__status--muted {
  color: var(--text-faint);
}

.test-card {
  display: grid;
  gap: 1rem;
  padding: 1.1rem;
}

.test-card__result {
  display: grid;
  gap: 0.55rem;
}

.test-card__result p,
.test-card__applied {
  margin: 0;
}

.test-card__result code {
  display: block;
  padding: 0.75rem 0.85rem;
  border-radius: 12px;
  background: var(--bg-code);
  color: var(--text-main);
  white-space: pre-wrap;
  word-break: break-all;
}

.test-card__error {
  color: var(--signal-red);
}

.test-card__muted {
  color: var(--text-faint);
}

.node-toolbar,
.node-card__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.65rem;
}

.node-toolbar {
  width: 100%;
  margin-top: 0.1rem;
}

.node-card {
  display: grid;
  gap: 1rem;
  padding: 1.1rem;
}

.node-card__header h3,
.node-card__eyebrow {
  margin: 0;
}

.node-card__eyebrow {
  color: var(--text-faint);
  font-size: 0.75rem;
}

.node-card__type {
  min-width: 10rem;
  min-height: 2.5rem;
  border-radius: 999px;
  border: 1px solid var(--border-subtle);
  padding: 0 0.9rem;
  background: var(--bg-elevated);
  color: var(--text-main);
}

.node-card__type:disabled {
  opacity: 0.62;
  cursor: not-allowed;
}

.node-section {
  display: grid;
  gap: 0.9rem;
}

.ghost-button {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-pill);
  padding: 0.58rem 0.95rem;
  background: var(--bg-elevated);
  color: var(--text-main);
  cursor: pointer;
  transition:
    background-color 180ms var(--curve-swift),
    border-color 180ms var(--curve-swift),
    transform 180ms var(--curve-swift);
}

.ghost-button:hover {
  background: var(--bg-soft);
  border-color: var(--border-strong);
}

.ghost-button--danger {
  color: var(--signal-red);
}

.panel-empty {
  padding: 1.15rem 1rem;
  border-radius: var(--radius-md);
  background: var(--bg-soft);
  color: var(--text-faint);
}

.panel-empty--wide {
  border: 1px dashed var(--border-subtle);
}

.review-panel__list {
  display: grid;
  gap: 0.95rem;
  margin: 1rem 0 0;
}

.review-panel {
  padding: 1.45rem 1.15rem 1.2rem;
}

.review-panel .rewrite-lab__panel {
  margin-top: 1.1rem;
  padding-top: 1.35rem;
}

.review-panel__list dt,
.review-panel__list dd {
  margin: 0;
}

.review-panel__list dt {
  color: var(--text-faint);
  font-size: 0.75rem;
  font-weight: 600;
}

.review-panel__list dd {
  margin-top: 0.35rem;
  color: var(--text-main);
}

.checklist {
  display: grid;
  gap: 0.55rem;
  margin: 0;
  padding-left: 1.1rem;
  color: var(--text-main);
}

.wizard-actions {
  display: flex;
  justify-content: end;
  flex-wrap: wrap;
  gap: 0.75rem;
}

.wizard-actions button {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-pill);
  padding: 0.68rem 1.05rem;
  background: var(--button-secondary-bg);
  color: var(--text-main);
  font-weight: 600;
  cursor: pointer;
  transition:
    background-color 180ms var(--curve-swift),
    border-color 180ms var(--curve-swift),
    transform 180ms var(--curve-swift);
}

.wizard-actions button:hover {
  background: var(--bg-soft);
  border-color: var(--border-strong);
}

.wizard-actions__primary {
  background: var(--button-primary-bg);
  border-color: transparent;
  color: #ffffff;
}

.wizard-actions__primary:hover {
  background: var(--button-primary-hover);
  color: #ffffff;
}

.wizard-actions button:disabled {
  cursor: not-allowed;
  opacity: 0.58;
}

.preview-stack {
  display: grid;
  gap: 0.95rem;
  margin-top: 1rem;
}

.preview-stack h2 {
  margin: 0;
  font-size: 1.12rem;
}

.step-panel-enter-active,
.step-panel-leave-active {
  transition:
    opacity 220ms var(--curve-swift),
    transform 220ms var(--curve-swift);
}

.step-panel-enter-from,
.step-panel-leave-to {
  opacity: 0;
  transform: translateY(12px);
}

@media (max-width: 1100px) {
  .mode-grid,
  .choice-list,
  .wizard-form--split {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 860px) {
  .rewrite-lab__intro,
  .node-stage__intro,
  .node-card__header,
  .node-section__head,
  .rewrite-card__head {
    flex-direction: column;
    align-items: stretch;
  }
}
</style>
