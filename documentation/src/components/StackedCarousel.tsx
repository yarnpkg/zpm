import LeftChevron from '@/assets/svg/left-chevron.svg?react';
import cn          from '@/utils/cn';
import {useState}  from 'preact/hooks';

interface Testimonial {
  id: number;
  quote: string;
  name: string;
  title: string;
  avatar: string;
}

const testimonials: Array<Testimonial> = [
  {
    id: 1,
    quote:
      `Yarn completely transformed the way we manage dependencies across our monorepo. The plugin system gave us the flexibility we never had before.`,
    name: `Alex Morgan`,
    title: `Lead Software Engineer at CodeNest`,
    avatar: `/placeholder.svg?height=60&width=60`,
  },
  {
    id: 2,
    quote:
      `The performance improvements we saw after switching were incredible. Our build times decreased by 40% and the developer experience is so much smoother.`,
    name: `Sarah Chen`,
    title: `Senior Frontend Developer at TechFlow`,
    avatar: `/placeholder.svg?height=60&width=60`,
  },
  {
    id: 3,
    quote:
      `Zero-installs changed everything for our team. No more waiting for npm install, just clone and start coding. It's a game changer for productivity.`,
    name: `Marcus Rodriguez`,
    title: `DevOps Engineer at CloudScale`,
    avatar: `/placeholder.svg?height=60&width=60`,
  },
  {
    id: 4,
    quote:
      `Zero-installs changed everything for our team. No more waiting for npm install, just clone and start coding. It's a game changer for productivity.`,
    name: `Marcus Rodriguez`,
    title: `DevOps Engineer at CloudScale`,
    avatar: `/placeholder.svg?height=60&width=60`,
  },
  {
    id: 5,
    quote:
      `Zero-installs changed everything for our team. No more waiting for npm install, just clone and start coding. It's a game changer for productivity.`,
    name: `Marcus Rodriguez`,
    title: `DevOps Engineer at CloudScale`,
    avatar: `/placeholder.svg?height=60&width=60`,
  },
  {
    id: 6,
    quote:
      `Zero-installs changed everything for our team. No more waiting for npm install, just clone and start coding. It's a game changer for productivity.`,
    name: `Marcus Rodriguez`,
    title: `DevOps Engineer at CloudScale`,
    avatar: `/placeholder.svg?height=60&width=60`,
  },
];

function getSlideStatus(index: number, current: number, total: number) {
  if (total === 0)
    return `hidden`;

  let diff = index - current;
  if (diff > total / 2)
    diff -= total;
  else if (diff < -total / 2)
    diff += total;

  if (diff === 0)
    return `active`;
  if (diff === 1 || (total === 2 && diff === -1))
    return `next`;
  if (diff === -1)
    return `prev`;
  if (diff === 2)
    return total === 4 ? `background-next` : `background-next`;
  if (diff === -2)
    return total === 4 ? `background-next` : `background-prev`;

  return `hidden`;
}

type NavButtonProps = {
  onClick: () => void;
  rotate?: boolean;
  className?: string;
};

function NavButton({onClick, rotate = false, className = ``}: NavButtonProps) {
  return (
    <div
      className={cn(
        `rounded-full size-12 flex items-center justify-center p-[1px] bg-linear-to-l from-white/15 to-white/5 shrink-0`,
        rotate ? `rotate-180` : ``,
        className,
      )}
    >
      <button
        type={`button`}
        onClick={onClick}
        className={`bg-linear-to-r from-[#181A1F] to-[#0D0F14] rounded-full w-full h-full flex items-center justify-center`}
        aria-label={`Slide navigation`}
      >
        <LeftChevron className={`size-5 fill-current text-white`} />
      </button>
    </div>
  );
}

export type StackedCarouselProps = {
  slides?: Array<Testimonial>;
};

export default function StackedCarousel({slides = testimonials}: StackedCarouselProps) {
  const numSlides = slides.length;
  const [current, setCurrent] = useState(0);

  if (numSlides === 0)
    return null;

  const prev = () => setCurrent((current - 1 + numSlides) % numSlides);
  const next = () => setCurrent((current + 1) % numSlides);

  return (
    <div className={`relative lg:flex lg:items-center lg:justify-center`}>
      <NavButton onClick={prev} className={`max-lg:hidden`} />

      <div id={`slider-container`} className={`relative w-full h-72 md:h-64`}>
        {slides.map(({id, quote, name, title}, i) => (
          <article
            key={id}
            data-status={getSlideStatus(i, current, numSlides)}
            className={`slide absolute w-full md:w-2/3 h-full inset-0 -translate-x-1/2 left-1/2 bg-linear-to-b from-white/15 to-white/5 p-[1px] rounded-[20px]`}
          >
            <div className={`bg-linear-to-b from-[#181A1F] to-[#0D0F14] backdrop-blur-[7.7px] rounded-[20px] p-6 flex flex-col justify-between h-full`}>
              <blockquote className={`text-lg md:text-xl leading-7 text-white line-clamp-5`}>
                “{quote}”
              </blockquote>
              <div className={`flex items-center gap-x-3 md:gap-x-4 pt-6 md:pt-7`}>
                <div className={`size-12 md:size-16 rounded-full bg-gray-400 shrink-0`}></div>
                <div>
                  <p className={`!text-sm md:!text-base text-white font-montserrat font-medium leading-[22px]`}>
                    {name}
                  </p>
                  <p className={`!text-sm md:!text-base font-montserrat leading-[22px] text-white/60 pt-1.5`}>
                    {title}
                  </p>
                </div>
              </div>
            </div>
          </article>
        ))}
      </div>

      <NavButton onClick={next} rotate className={`max-lg:hidden`} />

      <div className={`lg:hidden flex items-center justify-center gap-x-4 pt-8`}>
        <NavButton onClick={prev} />
        <NavButton onClick={next} rotate />
      </div>
    </div>
  );
}
