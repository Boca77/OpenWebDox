// Image UI for the demo: Insert image, the floating toolbar under a selected image (text
// wrapping, options, delete) and the Image options side panel, as in Google Docs.

const $ = (id) => document.getElementById(id);

const PT_PER_IN = 72;
const WRAPS = [
  ["inline", "In line"],
  ["wrap", "Wrap text"],
  ["break", "Break text"],
  ["behind", "Behind text"],
  ["front", "In front of text"],
];

/** 20×20 icons: the picture as a filled box, text as lines, arranged as each mode lays out. */
const WRAP_ICONS = {
  inline: '<path d="M3 4h14M3 16h14"/><rect x="3" y="7.5" width="5" height="5" class="fill"/><path d="M10 12h7"/>',
  wrap: '<rect x="3" y="6" width="6" height="8" class="fill"/><path d="M12 6h5M12 10h5M12 14h5M3 3h14M3 17h14"/>',
  break: '<path d="M3 3h14M3 17h14"/><rect x="6" y="6" width="8" height="8" class="fill"/>',
  behind: '<rect x="5" y="4" width="10" height="12" class="fill soft"/><path d="M3 6h14M3 10h14M3 14h14"/>',
  front: '<path d="M3 6h14M3 10h14M3 14h14"/><rect x="5" y="4" width="10" height="12" class="fill"/>',
};
const icon = (mode) =>
  `<svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">${WRAP_ICONS[mode]}</svg>`;

export function setUpImages(view) {
  setUpInsert(view);
  const bar = setUpToolbar(view);
  const panel = setUpPanel(view);
  const refresh = () => {
    bar.refresh();
    panel.refresh();
  };
  view.on("selection", refresh);
  view.on("edit", refresh);
  addEventListener("scroll", () => bar.refresh(), { passive: true });
  addEventListener("resize", () => bar.refresh());
  return refresh;
}

function setUpInsert(view) {
  const input = $("image-file");
  $("insert-image").addEventListener("click", () => input.click());
  input.addEventListener("change", async () => {
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    try {
      await view.insertImageFile(file);
    } catch (err) {
      alert(`Couldn't read that image: ${err.message ?? err}`);
    }
    view.focusEditor();
  });
}

/** The bar under the selected image, like Google Docs' image toolbar. */
function setUpToolbar(view) {
  const bar = $("image-bar");
  const trigger = $("wrap-trigger");
  const menu = $("wrap-menu");
  const items = WRAPS.map(([mode, label]) => {
    const li = document.createElement("li");
    li.setAttribute("role", "none");
    const b = document.createElement("button");
    b.type = "button";
    b.setAttribute("role", "menuitemradio");
    b.dataset.mode = mode;
    b.innerHTML = `${icon(mode)}<span>${label}</span><svg class="check" viewBox="0 0 20 20" width="18" height="18" aria-hidden="true"><path d="M4 10.5l4 4 8-9"/></svg>`;
    li.append(b);
    menu.append(li);
    return b;
  });

  // Toolbar clicks keep the image selected and the editor focused.
  bar.addEventListener("pointerdown", (e) => {
    if (e.target.closest("button")) e.preventDefault();
  });
  const openMenu = (focusFirst) => {
    menu.hidden = false;
    trigger.setAttribute("aria-expanded", "true");
    const current = items.find((b) => b.getAttribute("aria-checked") === "true") ?? items[0];
    if (focusFirst) current.focus();
  };
  const closeMenu = () => {
    menu.hidden = true;
    trigger.setAttribute("aria-expanded", "false");
  };
  trigger.addEventListener("click", () => (menu.hidden ? openMenu(true) : closeMenu()));
  menu.addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    closeMenu();
    view.setImageWrap(b.dataset.mode);
    view.focusEditor();
  });
  menu.addEventListener("keydown", (e) => {
    const i = items.indexOf(document.activeElement);
    if (e.key === "Escape") {
      closeMenu();
      trigger.focus();
    } else if (e.key === "ArrowDown") items[(i + 1) % items.length].focus();
    else if (e.key === "ArrowUp") items[(i - 1 + items.length) % items.length].focus();
    else return;
    e.preventDefault();
  });
  document.addEventListener("pointerdown", (e) => {
    if (!menu.hidden && !menu.contains(e.target) && !trigger.contains(e.target)) closeMenu();
  });
  $("image-options").addEventListener("click", () => {
    // Below the header, with the pages moved aside, as in Google Docs.
    $("image-panel").style.top = `${document.querySelector("header").getBoundingClientRect().bottom}px`;
    document.body.classList.add("panel-open");
    $("image-panel").hidden = false;
    $("image-options").setAttribute("aria-expanded", "true");
    $("image-panel").querySelector("details").open = true;
    view.focusEditor();
    refresh();
  });
  $("image-delete").addEventListener("click", () => {
    view.deleteImage();
    view.focusEditor();
  });

  const refresh = () => {
    const id = view.selectedImage;
    const rect = id ? view.imageClientRect() : undefined;
    const props = id ? view.imageProps() : undefined;
    if (!rect || !props) {
      bar.hidden = true;
      closeMenu();
      return;
    }
    bar.hidden = false;
    const wrap = props.inTable ? "inline" : props.wrap;
    trigger.querySelector(".current").innerHTML = icon(wrap);
    trigger.setAttribute("aria-label", `Text wrapping: ${WRAPS.find(([m]) => m === wrap)[1]}`);
    for (const b of items) {
      b.setAttribute("aria-checked", String(b.dataset.mode === wrap));
      // In a table cell an image is always in line.
      b.disabled = props.inTable && b.dataset.mode !== "inline";
    }
    // Below the image (above it if there's no room), kept on screen.
    const { width, height } = bar.getBoundingClientRect();
    const below = rect.bottom + 8;
    const top = below + height + 8 < innerHeight ? below : Math.max(8, rect.top - height - 8);
    const left = Math.min(Math.max(8, rect.left + rect.width / 2 - width / 2), innerWidth - width - 8);
    bar.style.top = `${top}px`;
    bar.style.left = `${left}px`;
  };
  return { refresh };
}

/** The Image options side panel. */
function setUpPanel(view) {
  const panel = $("image-panel");
  const close = () => {
    panel.hidden = true;
    document.body.classList.remove("panel-open");
    $("image-options").setAttribute("aria-expanded", "false");
    view.focusEditor();
  };
  $("panel-close").addEventListener("click", close);
  panel.addEventListener("keydown", (e) => {
    if (e.key === "Escape") close();
  });

  const num = (id) => Number($(id).value);
  const inches = (pt) => String(Math.round((pt / PT_PER_IN) * 100) / 100);

  // Size & rotation.
  let aspect = 1;
  const commitSize = (changed) => {
    const p = view.imageProps();
    if (!p) return;
    let w = num("img-width") * PT_PER_IN;
    let h = num("img-height") * PT_PER_IN;
    if ($("img-lock").checked) {
      if (changed === "w") h = w / aspect;
      else w = h * aspect;
    }
    if (w > 0 && h > 0) view.setImageSize(w, h);
  };
  $("img-width").addEventListener("change", () => commitSize("w"));
  $("img-height").addEventListener("change", () => commitSize("h"));
  $("img-scale").addEventListener("change", () => {
    const p = view.imageProps();
    const s = num("img-scale") / 100;
    if (p && s > 0 && p.pxWidth) view.setImageSize((p.pxWidth * 0.75) * s, (p.pxHeight * 0.75) * s);
  });
  $("img-reset-size").addEventListener("click", () => {
    const p = view.imageProps();
    if (p?.pxWidth) view.setImageSize(p.pxWidth * 0.75, p.pxHeight * 0.75);
  });
  $("img-rotation").addEventListener("change", () => view.setImageRotation(num("img-rotation")));
  $("img-rotate-90").addEventListener("click", () => {
    const p = view.imageProps();
    if (p) view.setImageRotation(p.rotation + 90);
  });

  // Text wrapping.
  const wrapGroup = $("img-wraps");
  for (const [mode, label] of WRAPS) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "wrap-choice";
    b.dataset.mode = mode;
    b.setAttribute("aria-pressed", "false");
    b.innerHTML = `${icon(mode)}<span>${label}</span>`;
    b.addEventListener("click", () => view.setImageWrap(mode));
    wrapGroup.append(b);
  }

  // Position.
  const commitOffset = () => view.setImageOffset(num("img-x") * PT_PER_IN, num("img-y") * PT_PER_IN);
  $("img-x").addEventListener("change", commitOffset);
  $("img-y").addEventListener("change", commitOffset);

  // Recolor.
  for (const b of panel.querySelectorAll("[data-recolor]")) {
    b.addEventListener("click", () => view.setImageRecolor(b.dataset.recolor));
  }

  // Adjustments: the picture follows the slider live; the document changes on release.
  const sliders = ["img-transparency", "img-brightness", "img-contrast"];
  const look = () => ({
    transparency: num("img-transparency"),
    brightness: num("img-brightness"),
    contrast: num("img-contrast"),
  });
  for (const id of sliders) {
    $(id).addEventListener("input", () => {
      $(`${id}-out`).textContent = $(id).value;
      view.previewImageLook(look());
    });
    $(id).addEventListener("change", () => {
      const l = look();
      view.setImageAdjustments(l.brightness, l.contrast, l.transparency);
    });
  }
  $("img-reset-adjust").addEventListener("click", () => view.setImageAdjustments(0, 0, 0));

  // Alt text.
  $("img-alt").addEventListener("change", () => view.setImageAlt($("img-alt").value));

  const refresh = () => {
    const p = view.imageProps();
    $("panel-empty").hidden = Boolean(p);
    $("panel-body").hidden = !p;
    if (!p) return;
    aspect = p.width / p.height;
    const active = document.activeElement;
    const set = (id, v) => {
      if ($(id) !== active) $(id).value = v;
    };
    set("img-width", inches(p.width));
    set("img-height", inches(p.height));
    set("img-scale", p.pxWidth ? String(Math.round((p.width / (p.pxWidth * 0.75)) * 100)) : "100");
    set("img-rotation", String(Math.round(p.rotation * 10) / 10));
    const wrap = p.inTable ? "inline" : p.wrap;
    for (const b of wrapGroup.children) {
      b.setAttribute("aria-pressed", String(b.dataset.mode === wrap));
      b.disabled = p.inTable && b.dataset.mode !== "inline";
    }
    const positioned = wrap !== "inline";
    $("position-section").classList.toggle("off", !positioned);
    for (const id of ["img-x", "img-y"]) $(id).disabled = !positioned;
    $("img-y").disabled = !positioned || wrap === "break";
    $("position-note").textContent = positioned
      ? "Measured from the left margin and the top of the paragraph the image is anchored to. It moves with that paragraph."
      : "In-line images sit in the text. Choose another wrapping to position this one.";
    set("img-x", inches(p.x));
    set("img-y", inches(p.y));
    for (const b of panel.querySelectorAll("[data-recolor]")) {
      b.setAttribute("aria-pressed", String(b.dataset.recolor === p.recolor));
    }
    for (const [id, v] of [["img-transparency", p.transparency], ["img-brightness", p.brightness], ["img-contrast", p.contrast]]) {
      set(id, String(Math.round(v)));
      $(`${id}-out`).textContent = String(Math.round(v));
    }
    set("img-alt", view.imageAlt());
  };
  return { refresh };
}
