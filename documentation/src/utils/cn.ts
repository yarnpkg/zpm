import {type ClassValue, clsx} from 'clsx';
import {twMerge}               from 'tailwind-merge';

// eslint-disable-next-line arca/no-default-export
export default function cn(...classes: Array<ClassValue>) {
  return twMerge(clsx(...classes));
}
