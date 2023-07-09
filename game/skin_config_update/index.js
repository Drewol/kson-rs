const definition = JSON.parse(
  Deno.readTextFileSync(`../skins/${Deno.args[0]}/config-definitions.json`),
);

const newDefinition = [];

for (const def of Object.entries(definition)) {
  if (typeof def[0] === "string" && typeof def[1] === "object") {
    if (def[0].startsWith("separator")) {
      newDefinition.push({ type: "separator" });
    } else if (def[1].type === "label") {
      newDefinition.push({ v: def[0], type: "label" });
    } else {
      newDefinition.push({ ...def[1], name: def[0] });
    }
  }
}

console.log(JSON.stringify(newDefinition, undefined, 2));
Deno.writeTextFileSync(
  `../skins/${Deno.args[0]}/config-definitions-updated.json`,
  JSON.stringify(newDefinition, undefined, 2),
  {
    create: true,
  },
);
