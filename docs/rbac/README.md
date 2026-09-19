# Kubernetes permissions

[terrace-reader.yaml](terrace-reader.yaml) grants the inventory's get, list and watch
permissions. Bind it to the identity in a dedicated kubeconfig. Applying a ClusterRole
creates it; a binding is what grants access. An administrator runs these commands after
replacing the user placeholder and checking the current Kubernetes context:

```sh
kubectl apply -f docs/rbac/terrace-reader.yaml
kubectl create clusterrolebinding terrace-reader --clusterrole=terrace-reader --user=YOUR_USER
```

Inventory and watch streams work with this role. It does not grant access to Secrets, pod
logs, exec, or the proxy subresources. Set the metrics interval to `0` if proxy metrics are
not wanted; leaving metrics enabled reports the refused reads without blocking inventory.

## Optional metrics proxy access

[terrace-metrics-proxy.yaml](terrace-metrics-proxy.yaml) is a separate, optional grant for
the kubelet summary and exporter endpoints. **It is not a read-only role.** Kubernetes
allows command execution through kubelet endpoints authorized by `get` on `nodes/proxy`.
Pod and Service proxies also reach application HTTP endpoints, whose GET handlers may
change state. RBAC cannot limit these grants to `/stats/summary` or `/metrics`.
See [Kubernetes API server bypass risks](https://kubernetes.io/docs/concepts/security/api-server-bypass-risks/).

Terrace itself only reads fixed metrics paths, but a stolen credential has all the permissions
its roles grant. Only bind the optional role if that additional access is acceptable:

```sh
kubectl apply -f docs/rbac/terrace-metrics-proxy.yaml
kubectl create clusterrolebinding terrace-metrics-proxy --clusterrole=terrace-metrics-proxy --user=YOUR_USER
```

For GPU metrics alone, an administrator can further scope the pod/service proxy permissions
to the exporter namespace with a Role and RoleBinding, and omit `nodes/proxy` entirely.

Existing installations that bound the older combined `terrace-reader` role should have their
administrator reapply the new base manifest and review whether any other role still grants
proxy access. This repository change does not alter existing cluster permissions by itself.
