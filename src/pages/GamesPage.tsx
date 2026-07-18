import { useMemo, useState } from "react";
import { AppIcon } from "../components/icons";
import type { Game } from "../lib/types";

export function GamesPage({ games, onCreate, onRename, onMerge }: {
  games: Game[];
  onCreate: (name: string) => Promise<void>;
  onRename: (game: Game, name: string) => Promise<void>;
  onMerge: (source: Game, target: Game) => Promise<void>;
}) {
  const [query, setQuery] = useState("");
  const visible = useMemo(() => games.filter((game) => [game.name, ...game.aliases].some((value) => value.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))), [games, query]);
  return (
    <div className="page-stack">
      <div className="section-title"><div><h2>Game classification</h2><span>{games.length}</span></div><button onClick={() => { const name = window.prompt("New game name", ""); if (name?.trim()) onCreate(name.trim()); }}><AppIcon name="add" size={16} /> New game</button></div>
      <label className="search-control game-search"><AppIcon name="search" size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search games and imported aliases" /></label>
      <section className="game-grid">
        {visible.map((game) => (
          <article className="game-card" key={game.id}>
            <span className="game-color large" style={{ background: game.color }}><AppIcon name="game" size={18} /></span>
            <div><strong>{game.name}</strong><small>{game.mediaCount} item{game.mediaCount === 1 ? "" : "s"}</small><p>{game.aliases.filter((alias) => alias.toLocaleLowerCase() !== game.name.toLocaleLowerCase()).slice(0, 3).join(" · ") || "No alternate labels"}</p></div>
            <div className="game-actions">
              <button title="Rename game" onClick={() => { const name = window.prompt("Rename game", game.name); if (name?.trim() && name.trim() !== game.name) onRename(game, name.trim()); }}><AppIcon name="notes" size={15} /> Rename</button>
              <button title="Merge into another game" disabled={games.length < 2} onClick={() => { const name = window.prompt("Merge into which game?", ""); const target = games.find((candidate) => candidate.name.toLocaleLowerCase() === name?.trim().toLocaleLowerCase()); if (target) onMerge(game, target); }}><AppIcon name="collection" size={15} /> Merge</button>
            </div>
          </article>
        ))}
      </section>
      {!visible.length ? <div className="empty-state"><AppIcon name="game" size={25} />No game classifications match.</div> : null}
    </div>
  );
}
