import { useEffect, useRef, useState, useCallback } from 'react';
import * as d3 from 'd3';
import { MagnifyingGlassPlus, MagnifyingGlassMinus } from '@phosphor-icons/react';

export interface StatsPoint {
  readonly time: Date;
  readonly value: number;
  readonly value2?: number;
}

interface Props {
  readonly data: readonly StatsPoint[];
  readonly color: string;
  readonly label: string;
  readonly formatY?: (v: number) => string;
  readonly color2?: string;
  readonly label2?: string;
  readonly yMax?: number;
}

export default function StatsChart({
  data,
  color,
  label,
  formatY = (v) => v.toFixed(1),
  color2,
  label2,
  yMax: forcedYMax,
}: Props) {
  const svgRef = useRef<SVGSVGElement>(null);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const zoomRef = useRef<d3.ZoomBehavior<SVGSVGElement, unknown> | null>(null);
  const [zoomTransform, setZoomTransform] = useState<d3.ZoomTransform>(d3.zoomIdentity);

  useEffect(() => {
    const svgEl = svgRef.current;
    if (svgEl == null) return;
    const svg = d3.select(svgEl) as d3.Selection<SVGSVGElement, unknown, null, undefined>;

    const zoom = d3.zoom<SVGSVGElement, unknown>()
      .scaleExtent([1, 20])
      .on('zoom', (event: d3.D3ZoomEvent<SVGSVGElement, unknown>) => {
        setZoomTransform(event.transform);
      });

    zoomRef.current = zoom;
    svg.call(zoom);
    svg.on('dblclick.zoom', null);

    return () => { svg.on('.zoom', null); };
  }, []);

  const handleZoomIn = useCallback(() => {
    const svgEl = svgRef.current;
    const zoom = zoomRef.current;
    if (svgEl == null || zoom == null) return;
    const svg = d3.select(svgEl) as d3.Selection<SVGSVGElement, unknown, null, undefined>;
    svg.transition().duration(250).call(zoom.scaleBy, 1.5);
  }, []);

  const handleZoomOut = useCallback(() => {
    const svgEl = svgRef.current;
    const zoom = zoomRef.current;
    if (svgEl == null || zoom == null) return;
    const svg = d3.select(svgEl) as d3.Selection<SVGSVGElement, unknown, null, undefined>;
    svg.transition().duration(250).call(zoom.scaleBy, 0.67);
  }, []);

  useEffect(() => {
    const svgEl = svgRef.current;
    const wrapper = wrapperRef.current;
    if (svgEl == null || wrapper == null || data.length < 2) return;

    const svg = d3.select(svgEl);
    svg.selectAll('*:not(defs)').remove();
    svg.select('defs').remove();

    const width = wrapper.clientWidth;
    const height = wrapper.clientHeight;
    const margin = { top: 6, right: 6, bottom: 22, left: 6 };
    const w = width - margin.left - margin.right;
    const h = height - margin.top - margin.bottom;

    const clipId = `clip-${label.replace(/\s/g, '-')}-${Math.random().toString(36).slice(2, 6)}`;
    svg.append('defs')
      .append('clipPath')
      .attr('id', clipId)
      .append('rect')
      .attr('width', w)
      .attr('height', h);

    const g = svg.append('g')
      .attr('transform', `translate(${margin.left},${margin.top})`);

    const xDomain = d3.extent(data, (d) => d.time) as [Date, Date];
    const xScale = d3.scaleTime().domain(xDomain).range([0, w]);
    const zoomedX = zoomTransform.rescaleX(xScale);

    const allVals = data.map((d) => d.value);
    if (label2) allVals.push(...data.map((d) => d.value2 ?? 0));
    const yMaxVal = forcedYMax ?? (d3.max(allVals) || 1) * 1.15;
    const yScale = d3.scaleLinear().domain([0, yMaxVal]).range([h, 0]);

    // Grid
    const gridG = g.append('g');
    gridG.call(
      d3.axisLeft(yScale).tickSize(-w).ticks(4).tickFormat(() => ''),
    );
    gridG.selectAll('line').attr('stroke', 'var(--border)').attr('stroke-opacity', 0.5);
    gridG.selectAll('.domain').remove();
    gridG.selectAll('text').remove();

    // Clipped content
    const content = g.append('g').attr('clip-path', `url(#${clipId})`);

    const drawSeries = (acc: (d: StatsPoint) => number, seriesColor: string) => {
      const gradient = svg.select('defs').append('linearGradient')
        .attr('id', `grad-${seriesColor.slice(1)}-${clipId}`)
        .attr('x1', '0%').attr('y1', '0%')
        .attr('x2', '0%').attr('y2', '100%');
      gradient.append('stop').attr('offset', '0%').attr('stop-color', seriesColor).attr('stop-opacity', 0.25);
      gradient.append('stop').attr('offset', '100%').attr('stop-color', seriesColor).attr('stop-opacity', 0.02);

      const area = d3.area<StatsPoint>()
        .x((d) => zoomedX(d.time))
        .y0(h)
        .y1((d) => yScale(acc(d)))
        .curve(d3.curveMonotoneX);

      content.append('path')
        .datum(data)
        .attr('d', area)
        .attr('fill', `url(#grad-${seriesColor.slice(1)}-${clipId})`);

      const line = d3.line<StatsPoint>()
        .x((d) => zoomedX(d.time))
        .y((d) => yScale(acc(d)))
        .curve(d3.curveMonotoneX);

      content.append('path')
        .datum(data)
        .attr('d', line)
        .attr('fill', 'none')
        .attr('stroke', seriesColor)
        .attr('stroke-width', 2);
    };

    drawSeries((d) => d.value, color);
    if (color2 && label2) {
      drawSeries((d) => d.value2 ?? 0, color2);
    }

    // X axis
    g.append('g')
      .attr('transform', `translate(0,${h})`)
      .call(d3.axisBottom(zoomedX).ticks(5).tickFormat(d3.timeFormat('%H:%M:%S') as unknown as (v: d3.NumberValue) => string))
      .call((axis) => {
        axis.select('.domain').remove();
        axis.selectAll('line').attr('stroke', 'var(--border)').attr('stroke-opacity', 0.3);
        axis.selectAll('text')
          .style('fill', 'var(--text-subtle)')
          .style('font-size', '10px')
          .style('font-family', 'var(--font-mono)');
      });

    // Y labels (inside top-right)
    g.append('g')
      .call(d3.axisRight(yScale).ticks(4).tickFormat((v: d3.NumberValue) => formatY(v as number)))
      .call((axis) => {
        axis.select('.domain').remove();
        axis.selectAll('.tick line').remove();
        axis.selectAll('text')
          .attr('x', w - 4)
          .attr('dy', -3)
          .style('text-anchor', 'end')
          .style('fill', 'var(--text-subtle)')
          .style('font-size', '9px')
          .style('font-family', 'var(--font-mono)')
          .style('opacity', '0.7');
      });

    // Invisible rect for zoom interaction (must be on top)
    g.append('rect')
      .attr('width', w)
      .attr('height', h)
      .attr('fill', 'none')
      .attr('pointer-events', 'all');

  }, [data, color, label, formatY, color2, label2, forcedYMax, zoomTransform]);

  return (
    <div className="relative w-full h-full" ref={wrapperRef}>
      <svg ref={svgRef} className="w-full h-full" />

      {/* Zoom controls */}
      <div className="absolute top-1 left-1 glass-subpanel flex items-center gap-0.5 p-0.5">
        <button
          onClick={handleZoomIn}
          className="h-6 w-6 inline-flex items-center justify-center rounded text-subtle-ui hover:text-main transition-colors"
          title="Zoom in"
        >
          <MagnifyingGlassPlus size={14} />
        </button>
        <button
          onClick={handleZoomOut}
          className="h-6 w-6 inline-flex items-center justify-center rounded text-subtle-ui hover:text-main transition-colors"
          title="Zoom out"
        >
          <MagnifyingGlassMinus size={14} />
        </button>
      </div>

      {/* Legend - offset above X-axis labels */}
      <div className="absolute bottom-6 left-1 glass-subpanel flex items-center gap-3 px-2 py-1 text-[10px]">
        <div className="flex items-center gap-1.5">
          <div className="w-2 h-2 rounded-full" style={{ background: color }} />
          <span className="text-subtle-ui">{label}</span>
        </div>
        {label2 != null && color2 != null && (
          <div className="flex items-center gap-1.5">
            <div className="w-2 h-2 rounded-full" style={{ background: color2 }} />
            <span className="text-subtle-ui">{label2}</span>
          </div>
        )}
      </div>
    </div>
  );
}
