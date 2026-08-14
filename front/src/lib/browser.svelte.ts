import type { Attachment } from "svelte/attachments";
import { InternalReactiveValue } from "./utils";
import { on } from "svelte/events";

export const visualViewportHeight = new InternalReactiveValue(
  window.visualViewport ? () => window.visualViewport?.height : () => undefined,
  (update) => {
    if (window.visualViewport) {
      on(window.visualViewport, "resize", update);
      on(window.visualViewport, "scroll", update);
    }
  },
);

// this is a svelte attachment cf. https://svelte.dev/docs/svelte/@attach
export function withCssVariables(variables: { [key: string]: string }): Attachment {
  return (element) => {
    for (const key in variables) {
      const value = variables[key];
      (element as HTMLElement)?.style.setProperty(`--${key}`, value);
    }
  };
}
