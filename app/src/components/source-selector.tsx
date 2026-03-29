import type { Profile } from "../types";

const ADD_NEW_VALUE = "__new__";

interface SourceSelectorProps {
  profiles: Profile[];
  activeProfileId: string | null;
  onSelect: (profileId: string) => void;
  onAddNew: () => void;
  thumbnail: string | null;
}

export function SourceSelector({
  profiles,
  activeProfileId,
  onSelect,
  onAddNew,
  thumbnail,
}: SourceSelectorProps) {
  const handleChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const val = e.target.value;
    if (val === ADD_NEW_VALUE) {
      onAddNew();
    } else {
      onSelect(val);
    }
  };

  const selectValue = activeProfileId ?? "";

  return (
    <div className="source-face">
      <label>Source Face</label>

      {profiles.length === 0 ? (
        <div className="source-no-profiles">
          <span className="placeholder">No profiles yet</span>
          <button className="btn-create-profile" onClick={onAddNew}>
            Create
          </button>
        </div>
      ) : (
        <select value={selectValue} onChange={handleChange}>
          <option value="" disabled>
            Select profile...
          </option>
          {profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
          <option value={ADD_NEW_VALUE}>+ Add New Profile</option>
        </select>
      )}

      {thumbnail ? (
        <img
          src={thumbnail}
          alt="source face preview"
          className="face-preview"
          width={128}
          height={128}
        />
      ) : activeProfileId ? (
        <div className="placeholder">Loading preview...</div>
      ) : null}
    </div>
  );
}
