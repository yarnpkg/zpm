import { useInstantSearch } from "react-instantsearch";

export default function PackageGridSkeleton() {
  const { status } = useInstantSearch();

  if (status === "loading" || status === "stalled") {
    return (
      <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-4 lg:gap-6 !mt-10 md:!mt-8">
        {Array.from({ length: 9 }).map((_, i) => (
          <div
            key={i}
            className="bg-linear-to-b from-white/15 to-white/5 rounded-[20px] p-px animate-pulse !mt-0"
          >
            <div className="p-6 bg-linear-to-b from-gray-950 to-gray-800 backdrop-blur-[5.7px] rounded-[20px] h-full">
              <div className="flex flex-col justify-between h-full">
                <div>
                  <div className="flex items-center gap-3 !mb-3">
                    <div className="h-7 w-full bg-white/20 rounded" />
                    <div className="h-5 w-10 bg-white/10 rounded" />
                  </div>
                  <div className="h-5 w-20 bg-white/10 rounded" />
                  <div className="!mt-3 flex flex-col gap-y-3">
                    <div className="h-4 w-full bg-white/10 rounded" />
                    <div className="h-4 w-5/6 bg-white/10 rounded" />
                    <div className="h-4 w-3/4 bg-white/10 rounded" />
                  </div>
                </div>
                <div className="flex items-center !mt-10 md:!mt-8 justify-between">
                  <div className="h-5 w-8 bg-white/10 rounded" />
                  <div className="h-5 w-8 bg-white/10 rounded" />
                </div>
              </div>
            </div>
          </div>
        ))}
      </div>
    );
  }

  return null;
}
