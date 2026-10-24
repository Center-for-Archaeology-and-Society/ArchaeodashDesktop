/**
 * "Save plot" for every chart: PNG or SVG, saved through the native save
 * dialog on the desktop (a download in a browser). Plotly charts render
 * through Plotly's own image export; hand-drawn SVG figures are serialized
 * with their computed colors and fonts inlined, so theme variables and
 * `currentColor` survive outside the app.
 */
import { useState, type ReactElement, type RefObject } from 'react';
import { downloadExportResult } from '../exports.ts';
import { BusyLabel } from './Busy.tsx';

export type ImageFormat = 'png' | 'svg';

const INLINE_PROPS = [
  'fill', 'fill-opacity', 'stroke', 'stroke-width', 'stroke-opacity', 'stroke-dasharray', 'stroke-linecap',
  'opacity', 'font-family', 'font-size', 'font-weight', 'font-style', 'text-anchor', 'dominant-baseline',
  'visibility', 'display',
] as const;

/** Safe file stem from a plot title. */
export function plotFileStem(name: string): string {
  const stem = name.replace(/[\\/:*?"<>|\u0000-\u001f]+/g, '_').trim().slice(0, 100);
  return stem || 'plot';
}

/** The page background, so saved PNGs are not transparent. */
function pageBackground(): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue('--surface').trim();
  return value || '#ffffff';
}

/** Standalone SVG text with computed styles inlined. */
export function serializeSvg(svg: SVGSVGElement): { text: string; width: number; height: number } {
  const clone = svg.cloneNode(true) as SVGSVGElement;
  const source = [svg, ...svg.querySelectorAll('*')];
  const target = [clone, ...clone.querySelectorAll('*')];
  source.forEach((el, i) => {
    const out = target[i] as SVGElement | undefined;
    if (!out || !(el instanceof Element)) return;
    const style = getComputedStyle(el);
    const inline = INLINE_PROPS.map((prop) => `${prop}:${style.getPropertyValue(prop)}`).join(';');
    out.setAttribute('style', `${inline};${out.getAttribute('style') ?? ''}`);
  });
  const box = svg.getBoundingClientRect();
  const viewBox = svg.viewBox?.baseVal;
  const width = Math.round(box.width || viewBox?.width || 800);
  const height = Math.round(box.height || viewBox?.height || 600);
  clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
  clone.setAttribute('width', String(width));
  clone.setAttribute('height', String(height));
  clone.style.background = pageBackground();
  return { text: new XMLSerializer().serializeToString(clone), width, height };
}

/** PNG (base64, no data-URL prefix) from SVG text, at `scale`× resolution. */
async function svgTextToPng(text: string, width: number, height: number, scale = 2): Promise<string> {
  const image = new Image();
  image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(text)}`;
  await image.decode();
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(width * scale));
  canvas.height = Math.max(1, Math.round(height * scale));
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('Canvas is unavailable.');
  ctx.fillStyle = pageBackground();
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL('image/png').split(',')[1] ?? '';
}

async function saveImage(name: string, format: ImageFormat, content: { svg?: string; pngBase64?: string }): Promise<boolean> {
  const stem = plotFileStem(name);
  if (format === 'svg') {
    return downloadExportResult({ file_name: `${stem}.svg`, media_type: 'image/svg+xml', content: content.svg ?? '' });
  }
  return downloadExportResult({ file_name: `${stem}.png`, media_type: 'image/png', content: content.pngBase64 ?? '', content_encoding: 'base64' });
}

/** Saves the first `<svg>` inside `container`. */
export async function saveSvgFigure(container: Element, name: string, format: ImageFormat): Promise<boolean> {
  const svg = container.querySelector('svg');
  if (!svg) throw new Error('There is no plot to save.');
  const { text, width, height } = serializeSvg(svg);
  return format === 'svg' ? saveImage(name, 'svg', { svg: text }) : saveImage(name, 'png', { pngBase64: await svgTextToPng(text, width, height) });
}

/** Saves a rendered Plotly chart. */
export async function savePlotlyChart(el: HTMLElement, name: string, format: ImageFormat): Promise<boolean> {
  const Plotly = await import('plotly.js-dist-min');
  const box = el.getBoundingClientRect();
  const width = Math.max(400, Math.round(box.width));
  const height = Math.max(300, Math.round(box.height));
  const url = await Plotly.toImage(el, { format, width, height, scale: format === 'png' ? 2 : 1 });
  if (format === 'svg') {
    const comma = url.indexOf(',');
    const body = url.slice(comma + 1);
    const svg = url.slice(0, comma).includes(';base64') ? atob(body) : decodeURIComponent(body);
    return saveImage(name, 'svg', { svg });
  }
  return saveImage(name, 'png', { pngBase64: url.split(',')[1] ?? '' });
}

export interface SvgPart {
  readonly text: string;
  readonly width: number;
  readonly height: number;
}

/** Decodes the SVG text of a Plotly `toImage` data URL. */
export function svgFromDataUrl(url: string): string {
  const comma = url.indexOf(',');
  const body = url.slice(comma + 1);
  return url.slice(0, comma).includes(';base64') ? atob(body) : decodeURIComponent(body);
}

/** Lays several SVG panels out in a grid as one SVG document. */
export function combineSvgParts(parts: readonly SvgPart[], columns: number, gap = 12): SvgPart {
  const cols = Math.max(1, Math.min(columns, parts.length));
  const cellW = Math.max(...parts.map((p) => p.width), 1);
  const cellH = Math.max(...parts.map((p) => p.height), 1);
  const rows = Math.ceil(parts.length / cols);
  const width = cols * cellW + (cols - 1) * gap;
  const height = rows * cellH + (rows - 1) * gap;
  const parser = new DOMParser();
  const doc = document.implementation.createDocument('http://www.w3.org/2000/svg', 'svg', null);
  const root = doc.documentElement;
  root.setAttribute('width', String(width));
  root.setAttribute('height', String(height));
  root.setAttribute('viewBox', `0 0 ${width} ${height}`);
  const bg = doc.createElementNS('http://www.w3.org/2000/svg', 'rect');
  bg.setAttribute('width', '100%');
  bg.setAttribute('height', '100%');
  bg.setAttribute('fill', pageBackground());
  root.append(bg);
  parts.forEach((part, i) => {
    const panel = parser.parseFromString(part.text, 'image/svg+xml').documentElement;
    panel.setAttribute('x', String((i % cols) * (cellW + gap)));
    panel.setAttribute('y', String(Math.floor(i / cols) * (cellH + gap)));
    root.append(doc.importNode(panel, true));
  });
  return { text: new XMLSerializer().serializeToString(root), width, height };
}

/** Saves an SVG document as SVG or PNG. */
export async function saveSvgPart(part: SvgPart, name: string, format: ImageFormat): Promise<boolean> {
  return format === 'svg'
    ? saveImage(name, 'svg', { svg: part.text })
    : saveImage(name, 'png', { pngBase64: await svgTextToPng(part.text, part.width, part.height) });
}

/** "Save PNG / Save SVG" buttons for the plot inside `target`. */
export function PlotSaveButtons({
  target,
  name,
  kind = 'svg',
  save,
}: {
  target?: RefObject<HTMLElement | null>;
  name: string;
  /** `plotly` for Plotly charts, `svg` for hand-drawn SVG figures. */
  kind?: 'plotly' | 'svg';
  /** Custom save (e.g. several panels combined); replaces the default. */
  save?: (format: ImageFormat) => Promise<unknown>;
}): ReactElement {
  const [busy, setBusy] = useState<ImageFormat | null>(null);
  const [error, setError] = useState('');
  const run = (format: ImageFormat) => {
    const el = target?.current;
    if (!save && !el) return;
    setBusy(format);
    setError('');
    const work = (): Promise<unknown> => {
      if (save) return save(format);
      const plot = kind === 'plotly' ? (el!.querySelector('.js-plotly-plot') as HTMLElement | null) ?? el! : el!;
      return kind === 'plotly' ? savePlotlyChart(plot, name, format) : saveSvgFigure(el!, name, format);
    };
    void work()
      .catch((cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause)))
      .finally(() => setBusy(null));
  };
  return (
    <div className="plot-save" role="group" aria-label={`Save ${name}`}>
      <button type="button" className="btn btn-sm btn-ghost" disabled={busy !== null} onClick={() => run('png')} title="Save the plot as a PNG image">
        <BusyLabel busy={busy === 'png'}>Save PNG</BusyLabel>
      </button>
      <button type="button" className="btn btn-sm btn-ghost" disabled={busy !== null} onClick={() => run('svg')} title="Save the plot as an SVG vector image">
        <BusyLabel busy={busy === 'svg'}>Save SVG</BusyLabel>
      </button>
      {error && <span role="alert" className="export-error">Could not save: {error}</span>}
    </div>
  );
}
