import React from 'react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { Sidebar } from './Sidebar';
import { I18nProvider } from '../i18n';
import { ConversationSummary } from '../types';
import { FOLDER_ORDER_STORAGE_KEY } from '../utils/folderOrder';

const conversation = (name: string): ConversationSummary => ({
  id: name, title: `${name} chat`, project_path: `C:\\${name}`, source_kind: 'codex_main',
  title_user_edited: false, archived: false, message_count: 1,
  created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
});
const conversations = ['Alpha', 'Beta', 'Gamma'].map(conversation);
const props: React.ComponentProps<typeof Sidebar> = {
  width: 300, conversations, selectedId: null, activeSource: 'codex',
  publishSelection: new Set(), folderPins: [], searchQuery: '',
  onSelectConversation: vi.fn(), onTogglePublishSelection: vi.fn(),
  onToggleProjectPublishSelection: vi.fn(), onClearSelection: vi.fn(),
  onBatchExportMarkdown: vi.fn(), onBatchDelete: vi.fn(), onOpenFolder: vi.fn(),
  onOpenVSCode: vi.fn(), onRename: vi.fn(), onDelete: vi.fn(), onSetPinned: vi.fn(),
  onSetFolderPinned: vi.fn(), onSearchChange: vi.fn(),
};
const sidebar = (overrides: Partial<typeof props> = {}) =>
  <I18nProvider><Sidebar {...props} {...overrides} /></I18nProvider>;
const folderButton = (name: string) => screen.getByRole('button', { name });
const folderNames = () => Array.from(document.querySelectorAll('[data-folder-sort-key]'))
  .map((element) => element.querySelector('[aria-expanded]')?.textContent);
const savedOrders = () => JSON.parse(localStorage.getItem(FOLDER_ORDER_STORAGE_KEY) ?? '{}');

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem('cv_locale', 'en');
  vi.clearAllMocks();
  // jsdom does not provide PointerEvent or pointer hit testing.
  vi.stubGlobal('PointerEvent', class extends MouseEvent {
    pointerId: number;
    isPrimary: boolean;
    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init);
      this.pointerId = init.pointerId ?? 1;
      this.isPrimary = init.isPrimary ?? true;
    }
  });
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: vi.fn() });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(document, 'elementFromPoint');
});

function pointAt(name: string, edge: 'before' | 'after') {
  const target = folderButton(name).closest<HTMLElement>('[data-folder-sort-key]')!;
  const header = target.querySelector<HTMLElement>('[data-folder-header]')!;
  vi.spyOn(header, 'getBoundingClientRect').mockReturnValue({
    top: 100, bottom: 130, height: 30, left: 0, right: 300, width: 300,
    x: 0, y: 100, toJSON: () => ({}),
  });
  vi.mocked(document.elementFromPoint).mockReturnValue(header);
  return { pointerId: 1, clientX: 40, clientY: edge === 'before' ? 105 : 125 };
}

function startDrag(name: string) {
  const button = folderButton(name);
  fireEvent.pointerDown(button, { button: 0, pointerId: 1, clientX: 40, clientY: 20 });
  return button;
}

function dragFolder(from: string, to: string, edge: 'before' | 'after') {
  const button = startDrag(from);
  const point = pointAt(to, edge);
  fireEvent.pointerMove(button, point);
  expect(document.querySelector('[data-folder-drop-indicator]')).toBeTruthy();
  fireEvent.pointerUp(button, point);
  fireEvent.click(button, { detail: 1 });
}

test('folders drag above and below neighbors without collapsing, and order survives reopening', () => {
  const view = render(sidebar());
  dragFolder('Gamma', 'Alpha', 'before');
  expect(folderNames()).toEqual(['Gamma', 'Alpha', 'Beta']);
  expect(folderButton('Gamma').getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByText('Gamma chat')).toBeTruthy();
  dragFolder('Gamma', 'Beta', 'after');
  expect(folderNames()).toEqual(['Alpha', 'Beta', 'Gamma']);
  dragFolder('Beta', 'Alpha', 'before');
  view.unmount();
  render(sidebar());
  expect(folderNames()).toEqual(['Beta', 'Alpha', 'Gamma']);
});

test('ordinary clicks still collapse folders and pin controls keep their existing behavior', () => {
  render(sidebar());
  const button = startDrag('Alpha');
  fireEvent.pointerMove(button, { pointerId: 1, clientX: 42, clientY: 21 });
  fireEvent.pointerUp(button, { pointerId: 1, clientX: 42, clientY: 21 });
  fireEvent.click(button, { detail: 1 });
  expect(screen.queryByText('Alpha chat')).toBeNull();
  expect(localStorage.getItem(FOLDER_ORDER_STORAGE_KEY)).toBeNull();
  fireEvent.click(screen.getAllByRole('button', { name: 'Pin folder' })[0]);
  expect(props.onSetFolderPinned).toHaveBeenCalledWith('C:\\Alpha', true);
});

test('source tabs keep separate orders and newly scanned folders follow saved folders', () => {
  const view = render(sidebar());
  dragFolder('Gamma', 'Alpha', 'before');
  view.rerender(sidebar({ activeSource: 'claude' }));
  expect(folderNames()).toEqual(['Alpha', 'Beta', 'Gamma']);
  dragFolder('Beta', 'Alpha', 'before');
  view.rerender(sidebar({ conversations: [...conversations, conversation('Delta')] }));
  expect(folderNames()).toEqual(['Gamma', 'Alpha', 'Beta', 'Delta']);
  view.rerender(sidebar({ activeSource: 'claude' }));
  expect(folderNames()).toEqual(['Beta', 'Alpha', 'Gamma']);
});

test('sorting filtered results preserves absent folders and their saved relative order', () => {
  const view = render(sidebar());
  dragFolder('Gamma', 'Alpha', 'before');
  view.rerender(sidebar({ conversations: [conversations[1], conversations[2]], searchQuery: 'filtered' }));
  dragFolder('Beta', 'Gamma', 'before');
  expect(savedOrders()['codex:folders']).toEqual(['folder:C:\\Beta', 'folder:C:\\Gamma', 'folder:C:\\Alpha']);
  view.rerender(sidebar());
  expect(folderNames()).toEqual(['Beta', 'Gamma', 'Alpha']);
});

test('Codex visible, hidden and pinned folders only reorder within their own section', () => {
  render(sidebar({
    conversations: [...conversations, conversation('Delta'), conversation('Epsilon')],
    folderPins: [{ source: 'codex', path: 'C:\\Epsilon', pinned_at: '2026-01-01' }],
    codexVisibility: {
      available: true,
      projects: ['Alpha', 'Beta'].map((name) => ({ project_id: name, name, root_paths: [`C:\\${name}`] })),
      conversations: {
        Alpha: { scope: 'project', project_id: 'Alpha' },
        Beta: { scope: 'project', project_id: 'Beta' },
      },
    },
  }));
  fireEvent.click(screen.getByRole('button', { name: /Not listed in Codex/ }));
  const button = startDrag('Gamma');
  const point = pointAt('Alpha', 'before');
  fireEvent.pointerMove(button, point);
  fireEvent.pointerUp(button, point);
  expect(localStorage.getItem(FOLDER_ORDER_STORAGE_KEY)).toBeNull();
  dragFolder('Beta', 'Alpha', 'before');
  dragFolder('Delta', 'Gamma', 'before');
  expect(folderNames()).toEqual(['Epsilon', 'Beta', 'Alpha', 'Delta', 'Gamma']);
  expect(Object.keys(savedOrders())).toEqual(['codex:visible', 'codex:hidden']);
});

test.each(['escape', 'cancel', 'outside', 'source change'])(
  '%s cancels a drag without changing saved order', (reason) => {
    const view = render(sidebar());
    const button = startDrag('Gamma');
    const point = pointAt('Alpha', 'before');
    fireEvent.pointerMove(button, point);
    if (reason === 'escape') fireEvent.keyDown(window, { key: 'Escape' });
    if (reason === 'cancel') fireEvent.pointerCancel(button, point);
    if (reason === 'outside') vi.mocked(document.elementFromPoint).mockReturnValue(null);
    if (reason === 'source change') view.rerender(sidebar({ activeSource: 'claude' }));
    fireEvent.pointerUp(button, point);
    expect(folderNames()).toEqual(['Alpha', 'Beta', 'Gamma']);
    expect(localStorage.getItem(FOLDER_ORDER_STORAGE_KEY)).toBeNull();
  }
);

test('keyboard sorting works and malformed stored preferences fall back to default order', () => {
  localStorage.setItem(FOLDER_ORDER_STORAGE_KEY, '{broken');
  const view = render(sidebar());
  expect(folderNames()).toEqual(['Alpha', 'Beta', 'Gamma']);
  fireEvent.keyDown(folderButton('Beta'), { altKey: true, key: 'ArrowUp' });
  expect(folderNames()).toEqual(['Beta', 'Alpha', 'Gamma']);
  fireEvent.keyDown(folderButton('Beta'), { altKey: true, key: 'ArrowDown' });
  expect(folderNames()).toEqual(['Alpha', 'Beta', 'Gamma']);
  view.unmount();
  localStorage.setItem(FOLDER_ORDER_STORAGE_KEY, JSON.stringify({ 'codex:folders': [42, null] }));
  render(sidebar());
  expect(folderNames()).toEqual(['Alpha', 'Beta', 'Gamma']);
});
