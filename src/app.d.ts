/// <reference types="@sveltejs/kit" />

declare namespace svelteHTML {
  interface HTMLAttributes<T> {
    onconsider?: (event: CustomEvent) => void;
    onfinalize?: (event: CustomEvent) => void;
  }
}
