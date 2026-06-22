import {RouterProvider}            from '@tanstack/react-router';
import {useMemo}                   from 'react';

import {PackagePageInner}          from './PackagePageInner';
import {PackageCtx}                from './contexts';
import {splatRoute, getRouter}     from './router';
import type {BrandIcons, Octicons} from './types';

splatRoute.update({component: PackagePageInner});

export default function PackagePage({brandIcons, octicons}: {brandIcons: BrandIcons, octicons: Octicons}) {
  const ctx = useMemo(() => ({brandIcons, octicons}), [brandIcons, octicons]);
  const router = useMemo(() => getRouter(), []);
  return (
    <PackageCtx.Provider value={ctx}>
      <RouterProvider router={router}/>
    </PackageCtx.Provider>
  );
}
