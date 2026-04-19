import {createRootRoute, createRoute, createRouter} from '@tanstack/react-router';

import {JobsLayout}                                 from './components/jobs-layout';
import {Layout}                                     from './components/layout';
import {DashboardRoute}                             from './routes/dashboard-route';
import {HistoryRoute}                               from './routes/history-route';

const rootRoute = createRootRoute({
  component: Layout,
});

const dashboardRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: `/`,
  component: DashboardRoute,
});

const jobsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: `/jobs`,
  component: JobsLayout,
});

const historyRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: `/history`,
  component: HistoryRoute,
});

const routeTree = rootRoute.addChildren([
  dashboardRoute,
  jobsRoute,
  historyRoute,
]);

export const router = createRouter({routeTree});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
