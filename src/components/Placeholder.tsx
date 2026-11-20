import type { ParentProps } from "solid-js";

/** Temporary page body used until each page's issue lands. */
export default function Placeholder(props: ParentProps<{ title: string; description: string }>) {
  return (
    <section class="page">
      <h1>{props.title}</h1>
      <p class="muted">{props.description}</p>
      <p class="badge">Em construção</p>
      {props.children}
    </section>
  );
}
