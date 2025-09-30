import Heading               from '@/components/Heading';

import NotFoundIllustration  from '../assets/svg/404.svg?react';
import StarsBackgroundMobile from '../assets/svg/stars-background-mobile.svg?react';
import StarsBackground       from '../assets/svg/stars-background.svg?react';


export default function ErrorPage() {
  return (
    <div class={`relative`}>
      <StarsBackgroundMobile class={`absolute top-1/2 -translate-1/2 left-1/2 -z-30 md:hidden`} />
      <StarsBackground class={`absolute top-1/2 -translate-1/2 left-1/2 -z-30 max-md:hidden`} />

      <NotFoundIllustration class={`absolute top-0 left-1/2 -translate-x-1/2 -z-20`} />

      <div class={`container pt-28 md:py-28 lg:pt-44`}>
        <div class={`not-content flex flex-col items-center justify-end text-center min-h-[400px]`}>
          <Heading size={`h1`}>Oops, you're lost in space</Heading>
          <p class={`leading-[22px] text-base !text-white/80 !mt-3 md:!mt-4`}>Page not found</p>

          <a
            href={`/`}
            class={`w-full px-5 py-4 text-white text-xs font-medium rounded-full md:w-fit md:text-sm border border-[#2F3F8C] bg-linear-to-b from-dark-100 to-blue-800/10 shadow-[inset_0px_0px_4.6px_1px_rgba(101,116,255,0.55),inset_0px_0px_12px_rgba(44,76,255,0.24)] !mt-6 md:!mt-10`}
          >
            Go home
          </a>
        </div>
      </div>
    </div>
  );
}
