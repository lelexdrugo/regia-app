// What a person reaches, as Chrome's accessibility tree has it: the proofs of the shell find a control by its role
// and its name, as a screen reader or a keyboard user would, and not by a class of the markup.

function property(node, name) {
  return node.properties?.find((entry) => entry.name === name)?.value?.value;
}

/** Every node of the page that is not ignored: role, name, and the states the proofs read. */
export async function tree(page) {
  const { nodes } = await page.send("Accessibility.getFullAXTree");
  return nodes
    .filter((node) => !node.ignored)
    .map((node) => ({
      role: node.role?.value ?? "",
      name: node.name?.value ?? "",
      focused: property(node, "focused") === true,
      checked: property(node, "checked"),
      level: property(node, "level"),
      modal: property(node, "modal") === true,
      backendNodeId: node.backendDOMNodeId,
    }));
}

/** The nodes with this role, and this name when one is given. */
export async function all(page, role, name) {
  return (await tree(page)).filter((node) => node.role === role && (name === undefined || node.name === name));
}

export async function find(page, role, name) {
  return (await all(page, role, name))[0];
}

/** The node that holds the focus, or none when it is on the page itself. */
export async function focused(page) {
  return (await tree(page)).find((node) => node.focused && node.role !== "RootWebArea");
}

/** "radio English", for the proofs' output. */
export function named(node) {
  return node ? `${node.role} ${JSON.stringify(node.name)}` : "nothing";
}

/** Moves the focus to `node`, as a click or a Tab would leave it. */
export async function focus(page, node) {
  await page.send("DOM.focus", { backendNodeId: node.backendNodeId });
}
