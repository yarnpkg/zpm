import { JsonDoc } from "react-json-doc";
import { useEffect } from "preact/hooks";
import { marked } from "marked";
import type { ReactNode } from "preact/compat";

const baseFont = {
  fontFamily: "Montserrat",
  fontStyle: "normal",
  fontWeight: "500",
  fontSize: "16px",
  lineHeight: "160%",
};

const jsonTheme = {
  plain: {
    ...baseFont,
  },
  styles: [
    {
      types: ["string"],
      style: {
        ...baseFont,
        color: "#FFB888",
        alignItems: "center",
      },
    },
    {
      types: ["keyword"],
      style: {
        color: "#FFFFFF",
      },
    },
    {
      types: ["attr-name"],
      style: {
        color: "#C3D2FF",
      },
    },
    {
      types: ["punctuation"],
      style: {
        color: "#FFFFFF99",
      },
    },
  ],
};

const extraTheme = {
  head: {
    padding: "24px",
    background: "rgba(42, 87, 219, 0.05)",
    border: "1px solid #7388FF",
    backdropFilter: "blur(4px)",
    borderRadius: "16px",
    ...baseFont,
    fontWeight: "400",
    color: "#FFFFFF",
  },
  inactiveHeader: {
    color: "#FFFFFF",
  },
  activeHeader: {
    background: "#3D437C",
    borderRadius: "16px",
  },
  annotation: {
    padding: "24px",
    gap: "16px",
    marginTop: "-6px",
    background: "rgba(255, 255, 255, 0.03)",
    border: "1.5px solid rgba(255, 255, 255, 0.05)",
    borderRadius: "16px",
  },
  anchor: {
    scrollMarginTop: 60,
  },
  section: {
    fontFamily: "Montserrat",
    fontWeight: "500",
  },
  identifier: {
    textDecoration: "underline",
    textUnderlineOffset: 3,
  },
};

export default function JsonToDoc({ json }: { json: string }) {
  useEffect(() => {
    const scrollToHash = () => {
      const raw =
        typeof window !== "undefined" ? window.location.hash.slice(1) : "";
      if (!raw) return;
      const id = decodeURIComponent(raw);
      const el = document.getElementById(id);
      if (el) {
        el.scrollIntoView({ block: "start" });
      }
    };

    // On initial mount after hydration
    scrollToHash();

    // Respond to in-page hash changes
    window.addEventListener("hashchange", scrollToHash);
    return () => window.removeEventListener("hashchange", scrollToHash);
  }, []);

  return (
    <JsonDoc
      linkComponent={({
        href,
        children,
      }: {
        href: string;
        children: ReactNode;
      }) => (
        <a href={href} className="!no-underline">
          {children}
        </a>
      )}
      data={json}
      theme={jsonTheme}
      extraTheme={extraTheme}
      descriptionRenderer={{
        render: (description: string) => (
          <div
            dangerouslySetInnerHTML={{
              __html: marked(description, { async: false }),
            }}
          />
        ),
      }}
    />
  );
}
