import SearchIcon                  from '@/assets/svg/search.svg?react';
import {navigate}                  from 'astro:transitions/client';
import {useLayoutEffect, useState} from 'preact/hooks';

export interface SearchInputProps {
  onQueryChange?: (query: string) => void;
}

function getQueryFromUrl() {
  const params = new URLSearchParams(location.search);
  return params.get(`q`) || ``;
}

export default function SearchInput({onQueryChange}: SearchInputProps) {
  const [query, setQuery] = useState(``);

  useLayoutEffect(() => {
    if (window.location.pathname === `/search`)
      setQuery(getQueryFromUrl());

    if (typeof navigation === `undefined`)
      return () => {};

    function handleUrlChange(e: NavigationEvent | null) {
      if (e.destination.pathname === `/`) {
        setQuery(``);
      }
    }

    navigation.addEventListener(`navigate`, handleUrlChange);

    return () => {
      navigation.removeEventListener(`navigate`, handleUrlChange);
    };
  }, []);

  function handleChange(value: string) {
    setQuery(value);
    onQueryChange?.(value);

    const targetUrl = value
      ? `/search?q=${encodeURIComponent(value)}`
      : `/search`;

    if (location.pathname === `/search`) {
      history.replaceState(null, ``, targetUrl);
    } else {
      navigate(targetUrl, {history: `push`});
    }
  }

  return (
    <div className={`rounded-full border border-white/15`}>
      <div className={`p-2 bg-linear-to-b from-transparent to-white/5 backdrop-blur-[2.5px] rounded-full drop-shadow-[0px 4px 26.8px rgba(17, 26, 59, 0.1)]`}>
        <div className={`absolute top-1/2 -translate-y-1/2 left-6 z-10`}>
          <SearchIcon className={`stroke-white size-6`} />
        </div>
        <input
          autoComplete={`off`}
          autoCorrect={`off`}
          autoCapitalize={`off`}
          placeholder={`Search packages (e.g. babel, webpack, react,...)`}
          spellcheck={false}
          maxLength={512}
          type={`search`}
          value={query}
          onChange={event => handleChange(event.currentTarget.value)}
          autoFocus
          className={`focus:outline-none w-full py-2 md:py-3 pl-12 pr-4 text-white placeholder:text-white/80 bg-white/[0.08] backdrop-blur-[3.4px] rounded-full placeholder:text-xs md:placeholder:text-sm placeholder:font-medium`}
        />
      </div>
    </div>
  );
}
