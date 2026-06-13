import {createRouter, createRoute, createRootRoute, useRouter} from '@tanstack/react-router';
import {useCallback}                                           from 'react';

const rootRoute = createRootRoute();

export const splatRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: `/package/$`,
});

export const routeTree = rootRoute.addChildren([splatRoute]);

let routerInstance: ReturnType<typeof createRouter> | null = null;

export function getRouter() {
  if (!routerInstance)
    routerInstance = createRouter({routeTree});

  return routerInstance;
}

declare module '@tanstack/react-router' {
  interface Register {
    router: ReturnType<typeof createRouter<typeof routeTree>>;
  }
}

export function usePackageNavigate() {
  const router = useRouter();
  return useCallback((path: string) => {
    router.history.push(path);
    router.load();
  }, [router]);
}
