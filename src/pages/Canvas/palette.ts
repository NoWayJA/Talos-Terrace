/** The node library. Dragging an entry onto the canvas plans a workload (apply arrives in M1). */
export const PALETTE_CATEGORIES = [
  { id: 'models', label: 'Model servers' },
  { id: 'apps', label: 'Apps' },
  { id: 'data', label: 'Data' },
  { id: 'generic', label: 'Generic' },
] as const;
export type PaletteCategoryId = (typeof PALETTE_CATEGORIES)[number]['id'];

export interface PaletteEntry {
  id: string;
  label: string;
  description: string;
  category: PaletteCategoryId;
  image: string;
  wantsGpu: boolean;
}

export const PALETTE: readonly PaletteEntry[] = [
  {
    id: 'ollama',
    label: 'Ollama',
    description: 'Model server with the OpenAI-compatible API',
    category: 'models',
    image: 'ollama/ollama',
    wantsGpu: true,
  },
  {
    id: 'llama-server',
    label: 'llama-server',
    description: 'llama.cpp HTTP server for GGUF models',
    category: 'models',
    image: 'ghcr.io/ggml-org/llama.cpp:server-cuda',
    wantsGpu: true,
  },
  {
    id: 'vllm',
    label: 'vLLM',
    description: 'High-throughput serving for safetensors models',
    category: 'models',
    image: 'vllm/vllm-openai',
    wantsGpu: true,
  },
  {
    id: 'open-webui',
    label: 'Open WebUI',
    description: 'Chat front end for any OpenAI-compatible server',
    category: 'apps',
    image: 'ghcr.io/open-webui/open-webui',
    wantsGpu: false,
  },
  {
    id: 'embeddings',
    label: 'Embeddings server',
    description: 'Text Embeddings Inference for retrieval',
    category: 'data',
    image: 'ghcr.io/huggingface/text-embeddings-inference',
    wantsGpu: true,
  },
  {
    id: 'pgvector',
    label: 'Postgres + pgvector',
    description: 'Relational store with vector search',
    category: 'data',
    image: 'pgvector/pgvector',
    wantsGpu: false,
  },
  {
    id: 'deployment',
    label: 'Deployment',
    description: 'Any image, one replica, Recreate when it wants the GPU',
    category: 'generic',
    image: '',
    wantsGpu: false,
  },
];
