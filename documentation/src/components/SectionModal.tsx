import {Dialog}              from '@base-ui-components/react/dialog';
import {useState, useEffect} from 'preact/hooks';
import CloseIcon             from 'src/assets/svg/close.svg?react';

const DialogRoot = Dialog.Root as any;
const DialogPortal = Dialog.Portal as any;
const DialogBackdrop = Dialog.Backdrop as any;
const DialogPopup = Dialog.Popup as any;
const DialogClose = Dialog.Close as any;

function isHeadingWrapper(element: HTMLElement | null): boolean {
  if (!element)
    return false;

  if (!element.classList.contains(`sl-heading-wrapper`))
    return false;

  return Array.from(element.classList).some(cls => /level-h[1-6]/.test(cls));
}

function collectSectionContentFromHeadingWrapper(
  startWrapper: HTMLElement,
): string {
  const container = document.createElement(`div`);
  let current: HTMLElement | null = startWrapper;

  while (current) {
    if (current !== startWrapper && isHeadingWrapper(current))
      break;

    container.appendChild(current.cloneNode(true));
    current = current.nextElementSibling as HTMLElement | null;
  }

  return container.innerHTML;
}

function findSectionFromHash(): string | null {
  const hash = window.location.hash.slice(1);
  if (!hash)
    return null;

  const target = document.getElementById(hash);
  if (!target)
    return null;

  let startWrapper: HTMLElement | null = target as HTMLElement | null;
  while (startWrapper && !isHeadingWrapper(startWrapper))
    startWrapper = startWrapper.previousElementSibling as HTMLElement | null;


  if (!startWrapper) {
    const allWrappers = Array.from(document.querySelectorAll(`.sl-heading-wrapper`))
      .filter(el => isHeadingWrapper(el as HTMLElement)) as Array<HTMLElement>;

    if (allWrappers.length === 0)
      return null;

    const preceding = allWrappers.filter(h =>
      (h.compareDocumentPosition(target) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0,
    );

    startWrapper = preceding.length
      ? preceding[preceding.length - 1]
      : allWrappers[0];
  }

  return collectSectionContentFromHeadingWrapper(startWrapper);
}

export default function SectionModal(): JSX.Element | null {
  const [modalContent, setModalContent] = useState<string | null>(null);

  useEffect(() => {
    const alreadyVisited = sessionStorage.getItem(`modalShown`);
    if (alreadyVisited)
      return;

    const html = findSectionFromHash();
    if (html) {
      setModalContent(html);
      sessionStorage.setItem(`modalShown`, `true`);
    }
  }, []);

  const handleOpenChange = (open: boolean) => {
    if (!open) {
      handleClose();
    }
  };

  const handleClose = () => {
    setModalContent(null);
  };

  return (
    <DialogRoot
      open={!!modalContent}
      onOpenChange={handleOpenChange}
    >
      <DialogPortal>
        <DialogBackdrop className={`fixed inset-0 bg-black/60 backdrop-blur-sm z-40`} />
        <DialogPopup className={`box-border fixed left-1/2 top-1/2 w-11/12 -translate-x-1/2 -translate-y-1/2 max-w-3xl max-h-[80vh] overflow-y-auto bg-linear-to-b from-gray-950 to-gray-800 rounded-xl p-6 z-50`}>
          <DialogClose
            aria-label={`Close dialog`}
            className={`absolute right-3 top-3 rounded-md px-2 py-1 text-xs text-white/80 hover:text-white focus:outline-none`}
          >
            <CloseIcon className={`size-5`} />
          </DialogClose>
          <div
            className={`sl-markdown-content`}
            dangerouslySetInnerHTML={{__html: modalContent || ``}}
          />
        </DialogPopup>
      </DialogPortal>
    </DialogRoot>
  );
}
