import type { JSX } from "preact";
import cn from "@/utils/cn";

type HeadingSizes = "h1" | "h2" | "h3" | "h4";

type HeadingProps = {
  children: preact.ComponentChildren;
  as?: string;
  size?: HeadingSizes;
  className?: string;
};

const sizeClasses = {
  h1: "text-[34px] lg:text-7xl tracking-[0.06em] lg:leading-[86px] leading-[41px] font-forma bg-linear-to-b from-[#656E98] to-white to-[87%] bg-clip-text text-transparent",
  h2: "font-forma md:!text-[64px] md:leading-[86px] tracking-[0.06em] bg-gradient-to-b from-[#656E98] to-white !text-transparent bg-clip-text !text-[34px] leading-[41px] font-normal",
  h3: "text-xl lg:text-[32px] tracking-normal leading-[38px] text-white font-medium",
  h4: "text-lg lg:text-xl tracking-normal leading-[28px] font-montserrat !text-white",
};

const Heading = ({
  children,
  as,
  size = "h3",
  className = "",
  ...props
}: HeadingProps) => {
  const Tag = (as || size) as keyof JSX.IntrinsicElements;

  return (
    <Tag class={cn(sizeClasses[size], className)} {...props}>
      {children}
    </Tag>
  );
};

export default Heading;
