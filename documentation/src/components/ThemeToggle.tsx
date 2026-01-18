import {useEffect, useState} from 'preact/hooks';

import MoonIcon from '@/assets/svg/moon.svg?react';
import SunIcon  from '@/assets/svg/sun.svg?react';

type Theme = `light` | `dark`;

function getInitialTheme(): Theme {
  if (typeof localStorage !== `undefined`) {
    const stored = localStorage.getItem(`theme`);
    if (stored === `light` || stored === `dark`)
      return stored;
  }
  if (typeof window !== `undefined` && window.matchMedia(`(prefers-color-scheme: light)`).matches)
    return `light`;

  return `dark`;
}

export default function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>(getInitialTheme);

  useEffect(() => {
    document.documentElement.setAttribute(`data-theme`, theme);
    localStorage.setItem(`theme`, theme);
  }, [theme]);

  useEffect(() => {
    const mediaQuery = window.matchMedia(`(prefers-color-scheme: light)`);
    const handleChange = (e: MediaQueryListEvent) => {
      if (!localStorage.getItem(`theme`))
        setTheme(e.matches ? `light` : `dark`);
    };

    mediaQuery.addEventListener(`change`, handleChange);
    return () => mediaQuery.removeEventListener(`change`, handleChange);
  }, []);

  const toggleTheme = () => {
    setTheme(prev => prev === `dark` ? `light` : `dark`);
  };

  return (
    <button
      type={`button`}
      onClick={toggleTheme}
      aria-label={theme === `dark` ? `Switch to light mode` : `Switch to dark mode`}
      className={`p-2 rounded-full hover:bg-white/10 transition-colors`}
    >
      {theme === `dark` ? (
        <SunIcon className={`size-5 text-white/90`} />
      ) : (
        <MoonIcon className={`size-5 text-gray-700`} />
      )}
    </button>
  );
}
