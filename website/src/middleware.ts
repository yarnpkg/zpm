import {defineMiddleware} from 'astro:middleware';

const browserPodHeaders = {
  'Cross-Origin-Embedder-Policy': `require-corp`,
  'Cross-Origin-Opener-Policy': `same-origin`,
};

export const onRequest = defineMiddleware(async (context, next) => {
  const response = await next();

  if (context.url.pathname.startsWith(`/playground`))
    for (const [name, value] of Object.entries(browserPodHeaders))
      response.headers.set(name, value);


  return response;
});
