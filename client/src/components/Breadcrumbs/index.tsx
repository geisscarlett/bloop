import {
  Fragment,
  MouseEvent,
  ReactElement,
  useEffect,
  useRef,
  useState,
} from 'react';
import { Range } from '../../types/results';
import BreadcrumbSection from './BreadcrumbSection';
import BreadcrumbsCollapsed from './BreadcrumbsCollapsed';

export type PathParts = {
  label: string;
  icon?: ReactElement<any, any>;
  link?: string;
  onClick?: (e: MouseEvent<HTMLButtonElement>) => void;
  highlight?: Range;
};

type Props = {
  pathParts: PathParts[];
  path: string;
  separator?: string;
  limitSectionWidth?: boolean;
  type?: 'link' | 'button';
  activeStyle?: 'primary' | 'secondary';
};

const Breadcrumbs = ({
  pathParts,
  separator = '/',
  type = 'link',
  limitSectionWidth,
}: Props) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [formattedPathParts, setFormattedPathParts] =
    useState<(PathParts | PathParts[])[]>(pathParts);

  useEffect(() => {
    if (!containerRef.current) {
      return;
    }
    const parentWidth = containerRef.current.parentElement!.clientWidth;
    const containerWidth = containerRef.current.scrollWidth;
    const widthDiff = containerWidth - parentWidth;
    if (widthDiff <= 0) {
      setFormattedPathParts(pathParts);
      return;
    }
    const partsToShow = [];
    let widthToHide = widthDiff;
    const parts = Array.from(containerRef.current.children).filter(
      (p) => p.getAttribute('data-role') !== 'separator',
    );
    for (let i = 0; i < pathParts.length; i++) {
      if (widthToHide <= 0) {
        partsToShow.push(...pathParts.slice(i));
        break;
      }
      if (i === 0 || pathParts[i].highlight || i === pathParts.length - 1) {
        partsToShow.push(pathParts[i]);
        continue;
      }
      if (Array.isArray(partsToShow[partsToShow.length - 1])) {
        // @ts-ignore just checked that it is an array above
        partsToShow[partsToShow.length - 1].push(pathParts[i]);
        widthToHide -= parts[i].clientWidth;
      } else {
        partsToShow.push([pathParts[i]]);
        widthToHide -=
          parts[i].clientWidth - Math.min(parts[i].clientWidth, 24);
      }
    }
    setFormattedPathParts(partsToShow);
  }, [pathParts]);

  return (
    <div className="flex items-center body-s flex-shrink-0 gap-1.5">
      {/* this div is hidden and used only to calculate the full width of breadcrumbs before truncation */}
      <div
        className="fixed top-full opacity-0 left-0 flex flex-nowrap items-center body-s flex-shrink-0 gap-1.5 select-none"
        ref={containerRef}
      >
        {pathParts.map((p, i) => (
          <Fragment key={i}>
            <span className={`flex items-center gap-1 flex-shrink-0`}>
              <BreadcrumbSection
                icon={p.icon}
                label={p.label}
                onClick={p.onClick}
                highlight={p.highlight}
                isLast={i == formattedPathParts.length - 1}
                type={type}
                limitSectionWidth={limitSectionWidth}
              />
            </span>
            {i !== formattedPathParts.length - 1 && (
              <span className="text-label-muted" data-role="separator">
                {separator}
              </span>
            )}
          </Fragment>
        ))}
      </div>
      {formattedPathParts.map((p, i) => (
        <Fragment key={i + (Array.isArray(p) ? 'array' : 'item')}>
          {Array.isArray(p) ? (
            <BreadcrumbsCollapsed items={p} type={type} />
          ) : (
            <span className={`flex items-center gap-1 flex-shrink-0`}>
              <BreadcrumbSection
                icon={p.icon}
                label={p.label}
                onClick={p.onClick}
                highlight={p.highlight}
                isLast={i == formattedPathParts.length - 1}
                type={type}
                limitSectionWidth={limitSectionWidth}
              />
            </span>
          )}
          {i !== formattedPathParts.length - 1 && (
            <span className="text-label-muted" data-role="separator">
              {separator}
            </span>
          )}
        </Fragment>
      ))}
    </div>
  );
};

export default Breadcrumbs;
