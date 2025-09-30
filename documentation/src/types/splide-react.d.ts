declare module "@splidejs/react-splide" {
  // Fallback typings compatible with Preact
  import type {FunctionComponent, ComponentChildren} from 'preact';

  export interface Options {
    [key: string]: unknown;
  }
  export interface SplideProps {
    options?: Options;
    hasTrack?: boolean;
    children?: ComponentChildren;
    ariaLabel?: string;
    [key: string]: unknown;
  }
  export const Splide: FunctionComponent<SplideProps> & {splide?: unknown};
  export const SplideSlide: FunctionComponent<{children?: ComponentChildren}>;
}
