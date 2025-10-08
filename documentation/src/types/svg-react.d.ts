declare module "*.svg?react" {
  import type {JSX}               from 'preact/jsx-runtime';
  import type {FunctionComponent} from 'preact';

  const content: FunctionComponent<JSX.SVGAttributes<SVGSVGElement>>;

  // eslint-disable-next-line arca/no-default-export
  export default content;
}
