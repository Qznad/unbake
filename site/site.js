// adds a copy button beside every code block, outside its scrolling area
function addCopyButtons() {
  for (const pre of document.querySelectorAll("pre")) {
    const box = document.createElement("div");
    box.className = "code";
    pre.before(box);
    box.appendChild(pre);
    const button = document.createElement("button");
    button.className = "copy";
    button.type = "button";
    button.textContent = "Copy";
    button.addEventListener("click", () => copyCode(pre, button));
    box.appendChild(button);
  }
}

// copies a code block's text and says so on its button
async function copyCode(pre, button) {
  const code = pre.querySelector("code") || pre;
  try {
    await navigator.clipboard.writeText(code.innerText.trim());
    button.textContent = "Copied";
  } catch {
    button.textContent = "Select and copy";
  }
  setTimeout(() => (button.textContent = "Copy"), 1600);
}

// replaces a player whose recording isn't published yet with a short note
function hideMissingAudio() {
  for (const audio of document.querySelectorAll("audio[data-pending]")) {
    audio.addEventListener("error", () => {
      const note = document.createElement("p");
      note.className = "pending";
      note.textContent = audio.dataset.pending;
      audio.replaceWith(note);
    });
    audio.load();
  }
}

addCopyButtons();
hideMissingAudio();
