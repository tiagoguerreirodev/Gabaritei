import { convertFileSrc } from "@tauri-apps/api/core";

/**
 * URL of a question image served by the backend's `image` scheme:
 * `image://localhost/<name>` on Linux and macOS, `http://image.localhost/<name>` on Windows.
 */
export function imageUrl(name: string) {
  return convertFileSrc(name, "image");
}

/** A question's image with its description for screen readers. */
export default function QuestionImage(props: { name: string; alt: string }) {
  return <img class="question-image" src={imageUrl(props.name)} alt={props.alt} loading="lazy" />;
}
