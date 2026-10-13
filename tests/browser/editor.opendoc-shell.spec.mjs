import { expect, test } from '@playwright/test';

async function boot(page, query = '') {
  await page.goto(`/editor.html${query}${query ? "&" : "?"}sample=demo`);
  await expect(page.locator('#tb-status')).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => document.fonts.ready);
}

test('OpenDoc ribbon is the default and the local Inter font loads', async ({ page }) => {
  await boot(page);
  await expect(page.locator('html')).toHaveClass(/oc-chrome-ribbon/);
  expect(await page.evaluate(() => document.fonts.check('13px Inter'))).toBe(true);
  const font = await page.locator('.app-header').evaluate(el => getComputedStyle(el).fontFamily);
  expect(font).toContain('Inter');
  await page.locator('.rb-file').click();
  await expect(page.locator('.rb-backstage')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.rb-backstage')).toBeHidden();
});

for (const width of [390, 768, 1440]) {
  test(`shell fits at ${width}px and tab switches leave the grid stable`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await boot(page);
    const phone = width <= 600;
    const before = await page.locator('#grid-wrap').boundingBox();
    for (const id of ['insert', 'data', 'home']) {
      if (phone) {
        // Phones use Excel mobile's bottom bar: the picker opens the tab chips.
        await page.locator('.rb-mpick').click();
        const chip = page.locator('.rb-mchip', { hasText: new RegExp(`^${id[0].toUpperCase()}${id.slice(1)}$`) });
        await chip.click();
        await expect(chip).toHaveAttribute('aria-pressed', 'true');
        await page.locator('.rb-mpick').click();
      } else {
        const tab = page.locator(`#rb-tab-${id}`);
        await tab.scrollIntoViewIfNeeded();
        await tab.click();
        await expect(tab).toHaveAttribute('aria-selected', 'true');
      }
      const after = await page.locator('#grid-wrap').boundingBox();
      expect(after.y).toBe(before.y);
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    const nav = await page.locator(phone ? '.rb-mpick' : '.rb-file').boundingBox();
    expect(nav.x).toBeGreaterThanOrEqual(0);
    expect(nav.x + nav.width).toBeLessThanOrEqual(width);
  });
}

test('skip link focuses the actual spreadsheet', async ({ page }) => {
  await boot(page);
  // Boot deliberately focuses the grid; focus the document's first link to
  // exercise its keyboard activation independently of that startup policy.
  await page.locator('.skip-link').focus();
  await expect(page.locator('.skip-link')).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(page.locator('#grid')).toBeFocused();
});

// One shell (UX-SHELL-08, docs/95): the toolbar and Sheets chromes are
// retired, so neither a URL nor a stored preference can bring them back.
test('every request draws the one Excel shell', async ({ page }) => {
  await boot(page, '?chrome=toolbar');
  await expect(page.locator('html')).toHaveClass(/oc-chrome-ribbon/);
  await page.evaluate(() => localStorage.setItem('oc.chrome', 'sheets'));
  await boot(page);
  await expect(page.locator('html')).toHaveClass(/oc-chrome-ribbon/);
  await expect(page.locator('html')).not.toHaveClass(/oc-chrome-sheets/);
});

test('hidden header keeps File and navigation accessible', async ({ page }) => {
  await boot(page, '?hide=header');
  await expect(page.locator('.app-header')).toBeHidden();
  await expect(page.locator('.rb-file')).toBeVisible();
  await page.locator('.rb-file').click();
  await expect(page.locator('.rb-backstage')).toBeVisible();
});

test('formatting uses the existing command and undo updates the ribbon', async ({ page }) => {
  await boot(page);
  await page.locator('#cell-ref').fill('B2');
  await page.locator('#cell-ref').press('Enter');
  const bold = page.locator('.oc-ribbon [data-oc-proxy="toolbar.bold"]');
  await expect(bold).toHaveAttribute('aria-pressed', 'false');
  await bold.click();
  await expect(bold).toHaveAttribute('aria-pressed', 'true');
  const undo = page.locator('.rb-qat [data-oc-proxy="edit.undo"]');
  await expect(undo).toBeEnabled();
  await undo.click();
  await expect(bold).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('#a11y-1-3')).toHaveText('13.5');
});

test('dark ribbon and File page follow the selected theme', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await boot(page);
  const colors = await page.locator('.rb-band').evaluate(el => {
    const root = getComputedStyle(document.documentElement);
    return { background: getComputedStyle(el).backgroundColor, text: root.getPropertyValue('--oc-text-color').trim() };
  });
  expect(colors.background).not.toBe('rgb(255, 255, 255)');
  expect(colors.text).toBe('#e7ebf0');
  await page.locator('.rb-file').click();
  await expect(page.locator('.rb-backstage')).toBeVisible();
  expect(await page.locator('.rb-backstage').evaluate(el => getComputedStyle(el).backgroundColor)).not.toBe('rgb(255, 255, 255)');
});
