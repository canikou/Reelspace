import { useMemo, useState } from "react";
import type { Game, MediaItem } from "../lib/types";
import { Dialog } from "./Dialog";
import { AppIcon } from "./icons";

export function GamePickerDialog({ item, games, onClose, onPick, onCreate }: {
  item: MediaItem;
  games: Game[];
  onClose: () => void;
  onPick: (game: Game) => Promise<void>;
  onCreate: (name: string) => Promise<Game>;
}) {
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const trimmedQuery = query.trim();
  const visible = useMemo(
    () => games
      .filter((game) => game.name.toLocaleLowerCase().includes(trimmedQuery.toLocaleLowerCase()))
      .slice(0, 30),
    [games, trimmedQuery],
  );
  const exactMatch = games.some(
    (game) => game.name.toLocaleLowerCase() === trimmedQuery.toLocaleLowerCase(),
  );

  async function choose(action: () => Promise<Game>) {
    setBusy(true);
    setError(null);
    try {
      await onPick(await action());
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "The game could not be assigned.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog title="Classify game" description={item.filename} onClose={busy ? () => undefined : onClose}>
      <div className="dialog-body game-picker-search">
        <AppIcon name="search" size={16} />
        <input
          autoFocus
          aria-label="Search games"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search or create a game"
        />
      </div>
      <div className="collection-picker">
        {visible.map((game) => (
          <button key={game.id} disabled={busy} onClick={() => choose(async () => game)}>
            <span className="game-color" style={{ background: game.color }} />
            <span>{game.name}</span>
            <small>{game.mediaCount}</small>
          </button>
        ))}
        {trimmedQuery && !exactMatch ? (
          <button disabled={busy} onClick={() => choose(() => onCreate(trimmedQuery))}>
            <AppIcon name="add" size={16} />
            <span>Create “{trimmedQuery}”</span>
          </button>
        ) : null}
        {!visible.length && !trimmedQuery ? <p className="muted picker-empty">No games yet</p> : null}
      </div>
      {error ? <p className="form-error" role="alert">{error}</p> : null}
    </Dialog>
  );
}
