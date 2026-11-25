/** Opens an attempt in the Resolver (works outside the router, e.g. from modals). */
export function openAttempt(id: string) {
  window.location.hash = `#/resolver?tentativa=${encodeURIComponent(id)}`;
}
