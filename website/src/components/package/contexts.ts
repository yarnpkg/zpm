import {createContext, useContext} from 'react';

import type {BrandIcons, Octicons} from './types';

export const IconCtx = createContext<{brand: BrandIcons, oct: Octicons}>(null!);

export function useIcons() {
  return useContext(IconCtx);
}

export const PackageCtx = createContext<{brandIcons: BrandIcons, octicons: Octicons}>(null!);
