/**
 * Upstream documentation links. Terrace is a general Talos console, so facts link to the Talos,
 * Kubernetes and vendor documentation, never to one project's own pages.
 *
 * The Talos documentation moved from talos.dev to docs.siderolabs.com in 2025 with a new
 * layout; `latest` redirects to the current version. `npm run links:check` verifies every
 * link here resolves (network; not part of `npm run check`).
 */
export interface DocLink {
  label: string;
  url: string;
}

const TALOS = 'https://docs.siderolabs.com/talos/latest';
const K8S = 'https://kubernetes.io/docs';

export const DOCS = {
  talosArchitecture: { label: 'Talos: architecture', url: `${TALOS}/learn-more/architecture` },
  talosSecureBoot: {
    label: 'Talos: Secure Boot',
    url: `${TALOS}/platform-specific-installations/bare-metal-platforms/secureboot`,
  },
  talosDiskEncryption: {
    label: 'Talos: disk encryption',
    url: `${TALOS}/configure-your-talos-cluster/storage-and-disk-management/disk-encryption`,
  },
  talosDisks: {
    label: 'Talos: disk management',
    url: `${TALOS}/configure-your-talos-cluster/storage-and-disk-management/disk-management/common`,
  },
  talosExtensions: {
    label: 'Talos: system extensions',
    url: `${TALOS}/build-and-extend-talos/custom-images-and-development/system-extensions`,
  },
  talosNetwork: { label: 'Talos: networking', url: `${TALOS}/learn-more/networking-resources` },
  talosUpgrade: {
    label: 'Talos: upgrading',
    url: `${TALOS}/configure-your-talos-cluster/lifecycle-management/upgrading-talos`,
  },
  talosNvidia: {
    label: 'Talos: NVIDIA GPU',
    url: `${TALOS}/configure-your-talos-cluster/hardware-and-drivers/nvidia-gpu`,
  },
  imageFactory: { label: 'Image Factory', url: 'https://factory.talos.dev/' },
  k8sNodes: { label: 'Kubernetes: nodes', url: `${K8S}/concepts/architecture/nodes/` },
  k8sNamespaces: {
    label: 'Kubernetes: namespaces',
    url: `${K8S}/concepts/overview/working-with-objects/namespaces/`,
  },
  k8sPodSecurity: {
    label: 'Kubernetes: Pod Security Admission',
    url: `${K8S}/concepts/security/pod-security-admission/`,
  },
  k8sDeployments: {
    label: 'Kubernetes: Deployments',
    url: `${K8S}/concepts/workloads/controllers/deployment/`,
  },
  k8sServices: {
    label: 'Kubernetes: Services',
    url: `${K8S}/concepts/services-networking/service/`,
  },
  k8sImages: { label: 'Kubernetes: images and digests', url: `${K8S}/concepts/containers/images/` },
  k8sEvents: {
    label: 'Kubernetes: events',
    url: `${K8S}/reference/kubernetes-api/cluster-resources/event-v1/`,
  },
  k8sDaemonSets: {
    label: 'Kubernetes: DaemonSets',
    url: `${K8S}/concepts/workloads/controllers/daemonset/`,
  },
  k8sStatefulSets: {
    label: 'Kubernetes: StatefulSets',
    url: `${K8S}/concepts/workloads/controllers/statefulset/`,
  },
  k8sJobs: { label: 'Kubernetes: Jobs', url: `${K8S}/concepts/workloads/controllers/job/` },
  k8sPods: { label: 'Kubernetes: Pods', url: `${K8S}/concepts/workloads/pods/` },
  k8sControlPlane: {
    label: 'Kubernetes: cluster architecture',
    url: `${K8S}/concepts/architecture/`,
  },
  kubevirtVms: {
    label: 'KubeVirt: virtual machines',
    url: 'https://kubevirt.io/user-guide/user_workloads/virtual_machine_instances/',
  },
  kubevirtAccess: {
    label: 'KubeVirt: reaching a VM',
    url: 'https://kubevirt.io/user-guide/user_workloads/accessing_virtual_machines/',
  },
  k8sGpus: {
    label: 'Kubernetes: scheduling GPUs',
    url: `${K8S}/tasks/manage-gpus/scheduling-gpus/`,
  },
  gpuOperator: {
    label: 'NVIDIA GPU Operator',
    url: 'https://docs.nvidia.com/datacenter/cloud-native/gpu-operator/latest/index.html',
  },
  ollamaApi: { label: 'Ollama API', url: 'https://github.com/ollama/ollama/blob/main/docs/api.md' },
} satisfies Record<string, DocLink>;
