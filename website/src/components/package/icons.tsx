import type {IconData} from './types';

export function OctIcon({icon, size = 16, className, style}: {icon: IconData, size?: number, className?: string, style?: React.CSSProperties}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${icon.width} ${icon.height}`}
      fill={`currentColor`}
      aria-hidden={`true`}
      className={className}
      style={style}
      dangerouslySetInnerHTML={{__html: icon.body}}
    />
  );
}

export function BrandIcon({icon, size = 14}: {icon: IconData, size?: number}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${icon.width} ${icon.height}`}
      fill={`currentColor`}
      aria-hidden={`true`}
      dangerouslySetInnerHTML={{__html: icon.body}}
    />
  );
}
