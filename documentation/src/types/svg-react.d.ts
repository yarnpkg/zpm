declare module "*.svg?react" {
  import type { FunctionComponent } from "preact";
  import type { JSX } from "preact/jsx-runtime";
  const content: FunctionComponent<JSX.SVGAttributes<SVGSVGElement>>;
  export default content;
}
