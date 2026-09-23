/**
 * Visibility note for the dataset table (Section 3.2): analytical_uuid is the
 * hidden immutable row identity — it addresses edits but is never displayed.
 */
export function HiddenIdNote(): React.JSX.Element {
  return (
    <p className="muted hidden-id-note">
      Row identities are managed internally; measured elemental columns are read-only.
      Descriptive cells can be edited and saved as a new revision.
    </p>
  );
}
