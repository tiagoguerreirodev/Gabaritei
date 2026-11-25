/** Copies JSON data so each occurrence of an object has its own identity. */
export function cloneJson<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}
