import { useMemo, useState } from "react";
import { Dialog } from "../components/Dialog";
import { AppIcon } from "../components/icons";
import { TextEditorDialog } from "../components/TextEditorDialog";
import { normalizeAppError } from "../lib/api";
import type { Game } from "../lib/types";

export function GamesPage({ games, onCreate, onRename, onMerge }: {
  games: Game[];
  onCreate: (name: string) => Promise<void>;
  onRename: (game: Game, name: string) => Promise<void>;
  onMerge: (source: Game, target: Game) => Promise<void>;
}) {
  const [query, setQuery] = useState("");
  const [dialog, setDialog] = useState<{ kind: "create" } | { kind: "rename" | "merge"; game: Game } | null>(null);
  const visible = useMemo(() => games.filter((game) => [game.name, ...game.aliases].some((value) => value.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))), [games, query]);
  return (
    <div className="page-stack">
      <div className="section-title"><div><h2>Game classification</h2><span>{games.length}</span></div><button onClick={() => setDialog({ kind: "create" })}><AppIcon name="add" size={16} /> New game</button></div>
      <label className="search-control game-search"><AppIcon name="search" size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search games and imported aliases" aria-label="Search games" /></label>
      <section className="game-grid">
        {visible.map((game) => (
          <article className="game-card" key={game.id}>
            <span className="game-color large" style={{ background: game.color }}><AppIcon name="game" size={18} /></span>
            <div><strong>{game.name}</strong><small>{game.mediaCount} item{game.mediaCount === 1 ? "" : "s"}</small><p>{game.aliases.filter((alias) => alias.toLocaleLowerCase() !== game.name.toLocaleLowerCase()).slice(0, 3).join(" · ") || "No alternate labels"}</p></div>
            <div className="game-actions">
              <button title="Rename game" onClick={() => setDialog({ kind: "rename", game })}><AppIcon name="notes" size={15} /> Rename</button>
              <button title="Merge into another game" disabled={games.length < 2} onClick={() => setDialog({ kind: "merge", game })}><AppIcon name="collection" size={15} /> Merge</button>
            </div>
          </article>
        ))}
      </section>
      {!visible.length ? <div className="empty-state"><AppIcon name="game" size={25} />{games.length ? "No games match this search." : "No games yet. Classify a recording or add one here."}</div> : null}
      {dialog?.kind === "create" ? (
        <TextEditorDialog title="New game" placeholder="Game name" onClose={() => setDialog(null)} onSave={async (name) => { if (!name.trim()) return; await onCreate(name.trim()); setDialog(null); }} />
      ) : null}
      {dialog?.kind === "rename" ? (
        <TextEditorDialog title="Rename game" description={dialog.game.name} initialValue={dialog.game.name} onClose={() => setDialog(null)} onSave={async (name) => { const next = name.trim(); if (next && next !== dialog.game.name) await onRename(dialog.game, next); setDialog(null); }} />
      ) : null}
      {dialog?.kind === "merge" ? (
        <MergeDialog source={dialog.game} games={games} onClose={() => setDialog(null)} onMerge={async (target) => { await onMerge(dialog.game, target); setDialog(null); }} />
      ) : null}
    </div>
  );
}

function MergeDialog({ source, games, onClose, onMerge }: { source: Game; games: Game[]; onClose: () => void; onMerge: (target: Game) => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function pick(target: Game) {
    setBusy(true);
    setError(null);
    try { await onMerge(target); } catch (reason) { setError(normalizeAppError(reason).message); setBusy(false); }
  }
  return (
    <Dialog title={`Merge “${source.name}” into…`} description="Its recordings and names move to the game you pick." onClose={busy ? () => undefined : onClose}>
      <div className="collection-picker">
        {games.filter((game) => game.id !== source.id).map((game) => (
          <button key={game.id} disabled={busy} onClick={() => pick(game)}>
            <span className="game-color" style={{ background: game.color }} />
            <span>{game.name}</span>
            <small>{game.mediaCount}</small>
          </button>
        ))}
      </div>
      {error ? <p className="form-error" role="alert">{error}</p> : null}
    </Dialog>
  );
}
