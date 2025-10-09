import cn                            from '@/utils/cn.ts';
import {isActivePath}                from '@/utils/helpers';
import * as DocSearchReact           from '@docsearch/react';
import {useEffect, useRef, useState} from 'preact/hooks';
import {createElement, render}       from 'preact';
import Close                         from 'src/assets/svg/close.svg?react';
import Discord                       from 'src/assets/svg/discord.svg?react';
import GitHub                        from 'src/assets/svg/github.svg?react';
import BrandLogo                     from 'src/assets/svg/logo.svg?react';
import Menu                          from 'src/assets/svg/menu.svg?react';
// Images
import SearchIcon                    from 'src/assets/svg/search-icon.svg?react';
import {NAVIGATION}                  from 'src/content/consts.ts';

import CollapsibleNavigation         from './CollapsibleNavigation.tsx';

const {DocSearchModal} = DocSearchReact;

export default function MobileNavigation({pathname}: {pathname: string}) {
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const searchContainerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null); // Ref for the menu container

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (
        isMenuOpen &&
        menuRef.current &&
        !menuRef.current.contains(event.target as Node)
      ) {
        setIsMenuOpen(false);
      }
    }

    if (isMenuOpen)
      document.addEventListener(`mousedown`, handleClickOutside);
    else
      document.removeEventListener(`mousedown`, handleClickOutside);


    return () => {
      document.removeEventListener(`mousedown`, handleClickOutside);
    };
  }, [isMenuOpen]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key === `k`) {
        event.preventDefault();
        setIsSearchOpen(true);
      }
    }
    document.addEventListener(`keydown`, onKeyDown);
    return () => document.removeEventListener(`keydown`, onKeyDown);
  }, []);

  useEffect(() => {
    const container = searchContainerRef.current;
    if (isSearchOpen && container) {
      const modalProps = {
        initialQuery: ``,
        appId: `STXW7VT1S5`,
        apiKey: `ecdfaea128fd901572b14543a2116eee`,
        indexName: `yarnpkg_next`,
        onClose: () => setIsSearchOpen(false),
        transformItems: (items: any) => items,
        placeholder: `Search documentation`,
        hitComponent: ({hit, children}: {hit: any, children: any}) => ({
          ...children,
          type: `a`,
          props: {
            children,
            href: hit.url,
            class: `!w-full !bg-transparent !hover:bg-transparent`,
          },
        }),
        navigator: {
          navigate({itemUrl}: {itemUrl: string}) {
            window.location.assign(itemUrl);
          },
        },
      };
      // @ts-expect-error: TOOD: Fix this
      render(createElement(DocSearchModal, modalProps), container);
    } else if (container) {
      render(null, container);
    }
    return () => {
      if (searchContainerRef.current) {
        render(null, searchContainerRef.current);
      }
    };
  }, [isSearchOpen]);

  return (
    <div
      className={cn(
        `bg-linear-to-b from-white/15 to-white/5 p-px`,
        isMenuOpen ? `rounded-2xl` : `rounded-full`,
      )}
    >
      <div
        className={cn(
          `px-4 py-3 bg-linear-to-b from-gray-950 to-gray-800`,
          isMenuOpen ? `rounded-2xl` : `rounded-full`,
        )}
      >
        <div className={`flex justify-between items-center`}>
          <a href={`/`} aria-label={`Logo`}>
            <BrandLogo class={`h-6 w-14 shrink-0`} />
          </a>

          <div className={`flex items-center gap-x-3`}>
            <div ref={searchContainerRef} id={`docsearch-container`} />

            <button
              type={`button`}
              id={`docsearch-button`}
              aria-label={`Search documentation`}
              onClick={() => setIsSearchOpen(true)}
            >
              <SearchIcon className={`size-6`} />
            </button>

            <button
              type={`button`}
              onClick={() => setIsMenuOpen(!isMenuOpen)}
              aria-label={`Toggle menu`}
            >
              {isMenuOpen ? (
                <Close className={`size-6 shrink-0`} />
              ) : (
                <Menu className={`size-6 shrink-0`} />
              )}
            </button>
          </div>
        </div>
        {isMenuOpen && (
          <div ref={menuRef} className={`pt-10 flex flex-col gap-y-10`}>
            <ul className={`flex flex-col gap-y-7`}>
              {NAVIGATION.map(
                ({href, title}: {href: string, title: string}) => (
                  <li key={href} onClick={() => setIsMenuOpen(false)}>
                    <a
                      href={href}
                      aria-label={title}
                      className={cn(
                        `font-medium text-sm leading-5 tracking-normal text-white/90 hover:text-[#7388FF]`,
                        isActivePath(pathname, href) && `text-[#7388FF] `,
                      )}
                    >
                      {title}
                    </a>
                  </li>
                ),
              )}
            </ul>

            <div className={`h-px w-full bg-white/15`}></div>

            <div className={`flex flex-col gap-y-6 pb-3`}>
              <CollapsibleNavigation />

              <div className={`flex items-center gap-x-6`}>
                <a
                  href={`https://discord.gg/yarnpkg`}
                  target={`_blank`}
                  rel={`noopener noreferrer`}
                >
                  <Discord className={`size-6 shrink-0`} />
                </a>
                <a
                  href={`https://github.com/yarnpkg/yarn`}
                  target={`_blank`}
                  rel={`noopener noreferrer`}
                >
                  <GitHub className={`size-6 shrink-0`} />
                </a>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
