import type { Profile } from "../types";

interface ProfileCardProps {
  profile: Profile;
  onSelect: (id: string) => void;
  onEdit: (id: string) => void;
}

const MAX_PHOTOS = 6;

export function ProfileCard({ profile, onSelect, onEdit }: ProfileCardProps) {
  const scorePercent = Math.round(profile.score * 100);
  const thumbnailSrc = profile.thumbnail_b64
    ? `data:image/jpeg;base64,${profile.thumbnail_b64}`
    : null;

  return (
    <div className="profile-card">
      <div className="profile-card-thumb">
        {thumbnailSrc ? (
          <img
            src={thumbnailSrc}
            alt={profile.name}
            className="thumbnail"
          />
        ) : (
          <div className="thumbnail thumbnail-placeholder">
            <span>No photo</span>
          </div>
        )}
        <div
          className="score-bar-wrap"
          title={`Detection confidence: ${scorePercent}%`}
        >
          <div
            className="score-bar"
            style={{ width: `${scorePercent}%` }}
          />
        </div>
      </div>

      <div className="profile-card-body">
        <div className="profile-card-name" title={profile.name}>
          {profile.name}
        </div>
        <div className="profile-card-meta">
          <span className="photo-count-badge">
            {profile.photo_count}/{MAX_PHOTOS}
          </span>
          <span className="score-label">{scorePercent}%</span>
        </div>
      </div>

      <div className="profile-card-actions">
        <button
          className="btn-use"
          onClick={() => onSelect(profile.id)}
          title="Use this profile for face swap"
        >
          Use
        </button>
        <button
          className="btn-edit"
          onClick={() => onEdit(profile.id)}
          title="Edit profile"
        >
          Edit
        </button>
      </div>
    </div>
  );
}
