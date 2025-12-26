import {marked}  from 'marked';
import {JsonDoc} from 'react-json-doc';

const baseFont = {
  fontFamily: `Montserrat`,
  fontStyle: `normal`,
  fontWeight: `500`,
  fontSize: `16px`,
  lineHeight: `160%`,
};

const jsonTheme = {
  plain: {
    ...baseFont,
  },
  styles: [
    {
      types: [`string`],
      style: {
        color: `#FFB888`,
        alignItems: `center`,
      },
    },
    {
      types: [`keyword`],
      style: {
        color: `#FFFFFF`,
      },
    },
    {
      types: [`attr-name`],
      style: {
        color: `#C3D2FF`,
      },
    },
    {
      types: [`punctuation`],
      style: {
        color: `#FFFFFF99`,
      },
    },
  ],
};

const extraTheme = {
  baseSize: `calc(var(--spacing) * 4)`,
  head: {
    background: `rgba(42, 87, 219, 0.05)`,
    border: `1px solid #7388FF`,
    backdropFilter: `blur(4px)`,
    borderRadius: `16px`,
    ...baseFont,
    fontWeight: `400`,
    color: `#FFFFFF`,
  },
  inactiveHeader: {
    color: `#FFFFFF`,
  },
  activeHeader: {
    background: `#3D437C`,
    borderRadius: `16px`,
  },
  annotation: {
    background: `rgba(255, 255, 255, 0.03)`,
    border: `1.5px solid rgba(255, 255, 255, 0.05)`,
    borderRadius: `16px`,
  },
  anchor: {
    scrollMarginTop: 60,
  },
  section: {
    fontFamily: `var(--font-mono)`,
    fontWeight: `500`,
  },
  identifier: {
    textDecoration: `underline`,
    textUnderlineOffset: 3,
  },
};

export default function JsonToDoc({json}: {json: string}) {
  return (
    <div class={`not-content json-doc`}>
      <JsonDoc
        data={json}
        theme={jsonTheme}
        extraTheme={extraTheme}
        descriptionRenderer={{
          render: (description: string) => (
            <div
              dangerouslySetInnerHTML={{
                __html: marked(description, {async: false}),
              }}
            />
          ),
        }}
      />
    </div>
  );
}
