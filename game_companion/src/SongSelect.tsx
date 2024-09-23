import { Accessor, Component, createEffect, For, Signal } from "solid-js";
import { ClientEvent, GameState } from "./schemas/types";

function range(s: number, e: number, v: number) {
  const r = [];
  for (var i = s; i <= e; i++) {
    r.push({ i, s: i == v });
  }

  return r;
}

export const SongSelect: Component<{
  state: Accessor<GameState & { variant: "SongSelect" }>;
  send: (m: ClientEvent) => void;
}> = (p) => {
  return (
    <div class="flex flex-col overflow-auto">
      <input
        placeholder="Search..."
        type="text"
        class="h-16 bg-slate-700 text-3xl w-full mb-5 px-2"
        onchange={(x) =>
          p.send({ variant: "SetSearch", v: x.currentTarget.value })
        }
      ></input>
      <div class="grid grid-cols-12 overflow-auto">
        <h2 class="col-span-2 text-2xl font-bold">Folders</h2>
        <h2 class="col-span-2 text-2xl font-bold">Levels</h2>
        <div class="col-span-8"></div>
        <div class="flex flex-col h-full items-start gap-3 p-2 col-span-2 overflow-auto relative">
          <For each={p.state().filters}>
            {(x, i) => {
              var name = "";
              switch (typeof x) {
                case "string":
                  name = x;
                  break;
                case "object":
                  if ("Folder" in x) {
                    name = x.Folder;
                  } else {
                    name = x.Collection;
                  }
                  break;
                default:
                  break;
              }

              return (
                <ListItem
                  name={name}
                  selected={p.state().folder_filter_index == i()}
                  onClick={() => p.send({ variant: "SetSongFilterType", v: x })}
                ></ListItem>
              );
            }}
          </For>
        </div>
        <div class="flex flex-col h-full items-start gap-3 p-2 col-span-2 overflow-auto relative">
          <ListItem
            name="None"
            selected={p.state().level_filter == 0}
            onClick={() => p.send({ variant: "SetLevelFilter", v: 0 })}
          ></ListItem>
          <For each={range(1, 20, p.state().level_filter)}>
            {(v) => (
              <ListItem
                name={"Level " + v.i}
                selected={v.s}
                onClick={() => p.send({ variant: "SetLevelFilter", v: v.i })}
              ></ListItem>
            )}
          </For>
        </div>
      </div>
    </div>
  );
};

const ListItem: Component<{
  name: string;
  selected: boolean;
  onClick: () => void;
}> = (p) => {
  return (
    <div
      onclick={p.onClick}
      class={
        "p-2 data-[sel=true]:bg-slate-700 data-[sel=true]:outline-2 data-[sel=true]:outline-amber-600 data-[sel=true]:outline bg-slate-800 w-full"
      }
      data-sel={p.selected}
    >
      {p.name}
    </div>
  );
};
