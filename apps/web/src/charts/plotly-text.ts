/**
 * Plotly draws trace names, text labels, hover text and tick labels as
 * pseudo-HTML (`<b>`, `<i>`, `<br>`, `<a href>`, `<span style>`, entities),
 * and expands `%{...}` inside hovertemplates. Sample IDs, metadata values,
 * group and column names are user data, so they are escaped before they
 * reach those strings; Plotly turns the entities back into the literal
 * characters when it renders the SVG text.
 *
 * Not for WebGL (`scattergl`) text labels: those are drawn as plain text and
 * would show the entities literally (their hover labels are SVG and still
 * need escaping).
 */
export function plotlyText(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/%/g, '&#37;');
}
