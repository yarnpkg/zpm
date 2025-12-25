import '@splidejs/react-splide/css';
import '@/styles/splide.css';
import cn                            from '@/utils/cn';
import {Splide, SplideSlide}         from '@splidejs/react-splide';
import {useRef, useEffect, useState} from 'preact/hooks';
// Images
import EllipseBlue                   from '@/assets/svg/ellipse-blue.svg?react';
import InlineSkeleton                from '@/assets/svg/inline-skeleton.svg?react';
import PolygonBlue                   from '@/assets/svg/polygon-blue.svg?react';
import ThreeRowSkeleton              from '@/assets/svg/three-row-skeleton.svg?react';

import type {CommandSlideProps}      from '../types/command';

interface CommandCardProps {
  slide: CommandSlideProps;
  isActive: boolean;
}

const DEFAULT_SLIDES: Array<CommandSlideProps> = [
  {
    command: `yarn set version stable`,
    name: `Yarn`,
    description:
      `It is an intermediary tool that will let you configure your package manager version on a per-project basis.`,
    link: `https://yarnpkg.com/cli/set/version`,
  },
  {
    command: `npm install -g corepack`,
    name: `Corepack`,
    description:
      `It is an intermediary tool that will let you configure your package manager version on a per-project basis.`,
    link: `https://yarnpkg.com/corepack`,
  },
  {
    command: `yarn init -2`,
    name: `Day.js`,
    description:
      `It is a lightweight JavaScript library for parsing, validating, manipulating, and formatting dates.`,
    link: `https://day.js.org/`,
  },
];

export default function CommandCarousel() {
  const splideRef = useRef<any>(null);
  const [activeIndex, setActiveIndex] = useState(1);

  useEffect(() => {
    const splide = splideRef.current?.splide;
    if (!splide)
      return () => {};

    const update = () => {
      const pagination = document.querySelector(`.splide__pagination`) as HTMLElement | null;
      const target = document.querySelector(`[data-pagination-target]`) as HTMLElement | null;

      if (pagination && target && !target.contains(pagination)) {
        target.appendChild(pagination);
      }
    };

    const handleMove = (newIndex: number) => {
      setTimeout(() => {
        setActiveIndex(newIndex);
      }, 300);
      update();
    };

    update();

    splide.on(`move`, handleMove);
    return () => {
      splide.off(`move`, handleMove);
    };
  }, []);

  return (
    <div className={`command-carousel overflow-hidden w-full relative`}>
      <Splide
        options={{
          start: 1,
          perPage: 1,
          gap: `24px`,
          arrows: false,
          autoplay: false,
          pagination: true,
          padding: {left: `32%`, right: `32%`},
          breakpoints: {
            1024: {
              padding: {left: `25%`, right: `25%`},
            },
            768: {
              gap: `16px`,
              padding: {left: `15%`, right: `15%`},
            },
            640: {
              padding: {left: `10%`, right: `10%`},
            },
          },
          speed: 500,
          easing: `cubic-bezier(0.25, 0.46, 0.45, 0.94)`,
        }}
        ref={splideRef}
        aria-label={`Command examples`}
        onMove={(_: unknown, index: number) => setActiveIndex(index)}
      >
        {DEFAULT_SLIDES.map((slide, index) => (
          <SplideSlide key={index}>
            <CommandCard slide={slide} isActive={index === activeIndex} />
          </SplideSlide>
        ))}
      </Splide>

      <div data-pagination-target aria-hidden={`true`} className={`mt-6`} />
    </div>
  );
}

function CommandCard({slide, isActive}: CommandCardProps) {
  return (
    <>
      <div className={`overflow-hidden min-w-[34vw] lg:min-w-[400px] transition-all duration-300`}>
        <div className={`mb-6`}>
          <div className={`relative overflow-hidden bg-linear-to-b from-white/15 to-white/5 p-px rounded-2xl lg:rounded-[20px] group`}>
            <div
              className={`absolute inset-0 w-full h-full z-10 pointer-events-none transition-opacity duration-500`}
              data-active-wrapper
            >
              <EllipseBlue
                className={cn(
                  `absolute inset-0 w-full h-full z-10 transition-all duration-500`,
                  isActive ? `opacity-100` : `opacity-30`,
                )}
              />
              <PolygonBlue
                className={cn(
                  `absolute -top-[86px] w-full h-full z-10 transition-all duration-500`,
                  isActive ? `opacity-100` : `opacity-0`,
                )}
              />
            </div>

            {slide.command.length > 20 ? (
              <>
                <InlineSkeleton className={`absolute top-1/2 -translate-y-1/2 h-42 w-auto opacity-30 left-0 -translate-x-2/3 md:-translate-x-1/2 2xl:-translate-x-1/4`} />
                <InlineSkeleton className={`absolute top-1/2 -translate-y-1/2 h-42 w-auto opacity-30 right-0 translate-x-2/3 md:translate-x-1/2 2xl:translate-x-1/4 rotate-180`} />
              </>
            ) : (
              <>
                <ThreeRowSkeleton className={`absolute top-1/2 -translate-y-1/2 h-42 w-auto left-0 2xl:translate-x-0 md:-translate-x-1/3 -translate-x-1/2`} />
                <ThreeRowSkeleton className={`absolute top-1/2 -translate-y-1/2 h-42 w-auto right-0 rotate-180 2xl:translate-x-0 md:translate-x-1/3 translate-x-1/2`} />
              </>
            )}

            <div className={`h-56 lg:h-64 bg-linear-to-b from-gray-950 to-gray-800 p-px rounded-2xl lg:rounded-[20px]`}>
              <div className={`absolute inset-0 flex items-center justify-center pointer-events-auto select-text z-20`}>
                <div className={`bg-linear-to-b from-white/15 to-white/5 p-1 rounded-xl`}>
                  <div className={`bg-linear-to-b from-white/15 to-white/5 p-1 rounded-xl`}>
                    <div className={`bg-linear-to-b from-dark-400 to-dark-200 px-4 py-3 lg:py-5 rounded-xl relative`}>
                      <div className={`flex items-start`}>
                        <p className={`leading-5 !font-etude !text-gray-50 text-xs lg:text-base overflow-x-auto whitespace-nowrap scrollbar-hide`}>
                          {slide.command}
                        </p>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div
        className={cn(
          `pt-6 lg:pt-10 text-center transition-all duration-500`,
          isActive
            ? `opacity-100 translate-y-0`
            : `opacity-0 translate-y-4 h-0 overflow-hidden`,
        )}
        role={`region`}
        aria-live={`polite`}
      >
        {slide.name && (
          <p className={`font-medium text-[#F4F0FF]! text-xl leading-7 mb-2`}>
            Install
            <a
              href={slide.link}
              target={`_blank`}
              rel={`noopener noreferrer`}
              className={`pl-1 text-[#7388FF] underline decoration-1 underline-offset-2 hover:text-[#8FA0FF] transition-colors`}
            >
              {slide.name}
            </a>
          </p>
        )}
        {slide.description && (
          <p className={`text-white/60! leading-5 max-md:text-sm pt-2 max-w-2xl mx-auto`}>
            {slide.description}
          </p>
        )}
      </div>
    </>
  );
}
