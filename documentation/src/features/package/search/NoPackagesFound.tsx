import NoPackages from 'src/assets/svg/no-packages-found.svg?react';

export default function NoPackagesFound() {
  return (
    <div className={`text-white/70 text-center !mt-15 flex w-full justify-center items-center flex-col gap-y-4 starting:opacity-0 transition opacity-100 duration-300`}>
      <NoPackages className={`max-w-full`} />
      <p className={`text-white/80 text-lg leading-[1.2] font-medium`}>
        No packages found
      </p>
    </div>
  );
}
