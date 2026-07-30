declare module 'svelte-dnd-action' {
  import type { Action } from 'svelte/action';

  export type DndEvent<T = unknown> = {
    items: T[];
    info: {
      trigger: string;
      id: string;
      source: string;
    };
  };

  export type DndZoneOptions = {
    items: Array<{ id: string | number } & Record<string, unknown>>;
    flipDurationMs?: number;
    dragDisabled?: boolean;
    dropTargetStyle?: Record<string, string> | null;
    morphDisabled?: boolean;
    type?: string;
  };

  export const dndzone: Action<HTMLElement, DndZoneOptions>;
  export const SOURCES: { POINTER: string; KEYBOARD: string };
  export const TRIGGERS: Record<string, string>;
}
