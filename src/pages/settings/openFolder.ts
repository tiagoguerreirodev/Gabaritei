import { api } from "../../api";
import type { OpenableFolder } from "../../bindings/OpenableFolder";
import { showError } from "../../components/toasts";

/** Opens one of the app's folders in the file manager; a failure shows as a toast. */
export async function openFolder(which: OpenableFolder): Promise<void> {
  try {
    await api.openFolder(which);
  } catch (error) {
    showError(error);
  }
}
