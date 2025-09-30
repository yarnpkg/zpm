import {navigate}                             from 'astro:transitions/client';
import {useEffect, useRef, useState}          from 'preact/hooks';
import {useSearchBox, type UseSearchBoxProps} from 'react-instantsearch';
import SearchIcon                             from 'src/assets/svg/search.svg?react';

export default function PackageSearchInput(props: UseSearchBoxProps) {
  const isHomePage = location.pathname === `/`;
  const {query, refine} = useSearchBox(props);

  const [inputValue, setInputValue] = useState(query);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();

    if (query && query.trim() !== ``) {
      localStorage.setItem(`lastSearchQuery`, query);
    }
  }, [query]);

  function setQuery(newQuery: string) {
    setInputValue(newQuery);
    refine(newQuery);

    if (!isHomePage)
      return;

    navigate(`/search?q=${encodeURIComponent(newQuery)}`, {
      history: `push`,
    });
  }

  return (
    <div className={`rounded-full border border-white/15`}>
      <div className={`p-2 bg-linear-to-b from-transparent to-white/5 backdrop-blur-[2.5px] rounded-full drop-shadow-[0px 4px 26.8px rgba(17, 26, 59, 0.1)]`}>
        <div className={`absolute top-1/2 -translate-y-1/2 left-6 z-10`}>
          <SearchIcon className={`stroke-white size-6`} />
        </div>
        <input
          ref={inputRef}
          autoComplete={`off`}
          autoCorrect={`off`}
          autoCapitalize={`off`}
          placeholder={`Search packages (e.g. babel, webpack, react,...)`}
          spellcheck={false}
          maxLength={512}
          type={`search`}
          value={inputValue}
          onChange={event => {
            setQuery(event.currentTarget.value);
          }}
          autoFocus
          className={`focus:outline-none w-full py-2 md:py-3 pl-12 pr-4 placeholder:text-white/80 bg-white/[0.08] backdrop-blur-[3.4px] rounded-full placeholder:text-xs md:placeholder:text-sm placeholder:font-medium`}
        />
      </div>
    </div>
  );
}
