/**
 * Import value handling, as in the original ArchaeoDash loader: how zero,
 * negative, and missing measured values and empty descriptive cells are
 * stored in the new group files. Defaults match the legacy app.
 */
import type { ReactElement } from 'react';
import type { ImportRecipeDto } from '@archaeodash/contracts';

export interface ImportValueOptionsState {
  readonly blankNonElement: boolean;
  readonly zeroAsNa: boolean;
  readonly negativeAsNa: boolean;
  readonly naAsZero: boolean;
}

/** Legacy defaults (DataLoader.R): blank label, zero and negative as NA. */
export const DEFAULT_VALUE_OPTIONS: ImportValueOptionsState = {
  blankNonElement: true,
  zeroAsNa: true,
  negativeAsNa: true,
  naAsZero: false,
};

/** Label written into empty descriptive cells, as in the legacy app. */
export const BLANK_LABEL = '[blank]';

export function recipeFromOptions(options: ImportValueOptionsState): ImportRecipeDto {
  return {
    zero_as_na: options.zeroAsNa,
    negative_as_na: options.negativeAsNa,
    na_as_zero: options.naAsZero,
    blank_non_element_label: options.blankNonElement ? BLANK_LABEL : null,
  };
}

export interface ImportValueOptionsProps {
  readonly value: ImportValueOptionsState;
  readonly onChange: (next: ImportValueOptionsState) => void;
  readonly disabled?: boolean;
}

export function ImportValueOptions({ value, onChange, disabled = false }: ImportValueOptionsProps): ReactElement {
  const box = (key: keyof ImportValueOptionsState, label: string) => (
    <label>
      <input type="checkbox" checked={value[key]} disabled={disabled} onChange={(event) => onChange({ ...value, [key]: event.target.checked })} /> {label}
    </label>
  );
  return (
    <fieldset className="import-wizard-values">
      <legend>Value handling</legend>
      {box('blankNonElement', `Replace empty/NA non-element fields with ${BLANK_LABEL}`)}
      {box('zeroAsNa', 'Treat zero values as NA')}
      {box('negativeAsNa', 'Treat negative values as NA')}
      {box('naAsZero', 'Replace NA values with 0')}
      <p>
        Zero, negative, and NA rules apply to the measured columns; replacing NA with 0 runs last.
        {value.naAsZero && (value.zeroAsNa || value.negativeAsNa) ? ' With both on, zeros and negatives end up as 0.' : ''}
      </p>
    </fieldset>
  );
}
