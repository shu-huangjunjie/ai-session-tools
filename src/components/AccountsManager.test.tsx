import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { I18nProvider } from '../i18n';
import { api } from '../api/client';
import { AccountsManager } from './AccountsManager';
import { AccountClient, AccountsState, AccountView } from '../types';

vi.mock('../api/client', () => ({
  api: {
    listAccounts: vi.fn(),
    saveCurrentAccount: vi.fn(),
    importAccountFile: vi.fn(),
    pickAuthJsonPath: vi.fn(),
    loginAccount: vi.fn(),
    cancelAccountLogin: vi.fn(),
    onAccountLoginUrl: vi.fn(),
    importAccountTokens: vi.fn(),
    switchAccount: vi.fn(),
    deleteAccount: vi.fn(),
    refreshAccountQuota: vi.fn(),
    refreshAllAccountQuotas: vi.fn(),
    upsertClaudeProvider: vi.fn(),
    importCcSwitchClaudeProviders: vi.fn(),
  },
  errorMessage: (e: unknown) => String(e),
}));

const inHours = (h: number) => new Date(Date.now() + h * 3600000).toISOString();
const account = (id: string, extra: Partial<AccountView> = {}): AccountView => ({
  id, is_current: false, has_quota: true, added_at: '2026-10-01T00:00:00Z', ...extra,
});

const STATES: Record<AccountClient, () => AccountsState> = {
  codex: () => ({
    auth_path: 'C:\\Users\\Demo\\.codex\\auth.json',
    current: { email: 'work@example.com', plan_type: 'pro', saved: true },
    notes: [],
    accounts: [
      account('cx-a', {
        email: 'work@example.com', plan_type: 'pro', is_current: true,
        quota: { plan_type: 'pro', windows: [
          { kind: 'primary', used_percent: 38, window_minutes: 300, resets_at: inHours(2.5) },
          { kind: 'secondary', used_percent: 93, window_minutes: 10080, resets_at: inHours(72) },
        ] },
      }),
      account('cx-b', { email: 'personal@example.com', quota_error: 'token expired, sign in again' }),
    ],
  }),
  claude: () => ({
    auth_path: 'C:\\Users\\Demo\\.claude',
    current: { kind: 'provider', name: 'Relay One', saved: true },
    notes: ['a relay overrides the official login'],
    accounts: [
      account('cl-o', {
        kind: 'oauth', email: 'me@example.com', plan_type: 'max',
        quota: { windows: [{ kind: 'seven_day_opus', label: 'Opus', used_percent: 55, window_minutes: 10080 }] },
      }),
      account('cl-r', { kind: 'provider', name: 'Relay One', detail: 'https://relay.one · m1', is_current: true, has_quota: false }),
      account('cl-r2', { kind: 'provider', name: 'Relay Two', detail: 'https://relay.two', has_quota: false }),
    ],
  }),
  antigravity: () => ({
    auth_path: 'C:\\state.vscdb',
    current: { email: 'g@gmail.com', saved: true },
    notes: [],
    accounts: [
      account('ag-1', { email: 'g@gmail.com', is_current: true, quota: { windows: [{ kind: 'gemini', label: 'Gemini 3 Pro', used_percent: 80 }] } }),
      account('ag-2', { email: 'h@gmail.com' }),
    ],
  }),
};

const toast = vi.fn();
const renderManager = () =>
  render(
    <I18nProvider>
      <AccountsManager onToast={toast} />
    </I18nProvider>
  );

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  localStorage.setItem('cv_locale', 'en');
  vi.mocked(api.listAccounts).mockImplementation(async (client) => STATES[client]());
  vi.mocked(api.switchAccount).mockResolvedValue(true);
  vi.mocked(api.refreshAllAccountQuotas).mockResolvedValue([]);
  vi.mocked(api.refreshAccountQuota).mockResolvedValue(undefined);
  vi.mocked(api.deleteAccount).mockResolvedValue(undefined);
  vi.mocked(api.upsertClaudeProvider).mockResolvedValue('new');
  vi.mocked(api.importCcSwitchClaudeProviders).mockResolvedValue(2);
  vi.mocked(api.onAccountLoginUrl).mockResolvedValue(() => {});
  vi.mocked(api.cancelAccountLogin).mockResolvedValue(undefined);
});

afterEach(cleanup);

const currentCard = () => screen.getByRole('region', { name: 'Current login' });

test('the current login card shows its quota, and other accounts are cards with errors', async () => {
  renderManager();
  const personal = await screen.findByRole('listitem', { name: 'personal@example.com' });
  expect(api.listAccounts).toHaveBeenCalledWith('codex');
  const current = currentCard();
  expect(within(current).getByText('work@example.com')).toBeTruthy();
  expect(within(current).getByText('5 hours')).toBeTruthy();
  expect(within(current).getByText('38% used')).toBeTruthy();
  expect(within(current).getAllByRole('progressbar').map((b) => b.getAttribute('aria-valuenow'))).toEqual(['38', '93']);
  expect(within(current).getByText(/resets in 2\.5 h/)).toBeTruthy();
  // The current account is not repeated among the others, and needs no switch button.
  expect(screen.queryByRole('listitem', { name: 'work@example.com' })).toBeNull();
  expect(within(current).queryByRole('button', { name: 'Switch' })).toBeNull();
  expect(within(personal).getByText('token expired, sign in again')).toBeTruthy();
  // Codex accounts are all subscriptions, current login included.
  expect(within(screen.getByRole('region', { name: 'Official subscriptions' })).getByRole('region', { name: 'Current login' })).toBeTruthy();
  expect(screen.queryByRole('region', { name: 'Relay providers' })).toBeNull();
});

test('each tab loads and switches only its own client', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(within(await screen.findByRole('listitem', { name: 'personal@example.com' })).getByRole('button', { name: 'Switch' }));
  expect(api.switchAccount).toHaveBeenCalledWith('codex', 'cx-b', true);

  await user.click(screen.getByRole('tab', { name: 'Claude' }));
  const oauth = await screen.findByRole('listitem', { name: 'me@example.com' });
  expect(api.listAccounts).toHaveBeenCalledWith('claude');
  expect(screen.queryByText('work@example.com')).toBeNull();
  expect(screen.getByText('a relay overrides the official login')).toBeTruthy();
  expect(within(oauth).getByText('Weekly · Opus')).toBeTruthy();
  // Subscriptions and relays are listed apart.
  expect(within(screen.getByRole('region', { name: 'Official subscriptions' })).getAllByRole('listitem').map((x) => x.getAttribute('aria-label'))).toEqual(['me@example.com']);
  expect(within(screen.getByRole('region', { name: 'Relay providers' })).getAllByRole('listitem').map((x) => x.getAttribute('aria-label'))).toEqual(['Relay Two']);
  // The current relay sits in the relay group.
  expect(within(screen.getByRole('region', { name: 'Relay providers' })).getByRole('region', { name: 'Current login' })).toBeTruthy();
  // Claude Code has nothing to restart, so the option is not offered.
  expect(screen.queryByRole('switch')).toBeNull();
  await user.click(within(oauth).getByRole('button', { name: 'Switch' }));
  expect(api.switchAccount).toHaveBeenLastCalledWith('claude', 'cl-o', false);

  const relay = currentCard();
  expect(within(relay).getByText('Relay One')).toBeTruthy();
  expect(within(relay).getByText('Relays have no common quota API')).toBeTruthy();
  expect(within(relay).queryByRole('button', { name: 'Refresh quota' })).toBeNull();

  await user.click(screen.getByRole('tab', { name: 'Antigravity' }));
  await screen.findByRole('listitem', { name: 'h@gmail.com' });
  // Models are grouped by family; the full list sits behind a toggle.
  const pro = within(currentCard()).getByRole('progressbar', { name: 'Gemini Pro' });
  expect([pro.getAttribute('aria-valuenow'), pro.getAttribute('aria-valuetext')]).toEqual(['20', '20% left']);
  expect(within(currentCard()).queryByText('3 Pro')).toBeNull();
  await user.click(within(currentCard()).getByRole('button', { name: 'All 1 models' }));
  expect(within(currentCard()).getByText('3 Pro')).toBeTruthy();
  await user.click(screen.getByRole('switch', { name: 'Close and reopen Antigravity when switching' }));
  await user.click(within(screen.getByRole('listitem', { name: 'h@gmail.com' })).getByRole('button', { name: 'Switch' }));
  expect(api.switchAccount).toHaveBeenLastCalledWith('antigravity', 'ag-2', false);
  // The Codex restart choice is separate and still on.
  expect(localStorage.getItem('cv_antigravity_restart_after_switch')).toBe('false');
  expect(localStorage.getItem('cv_codex_restart_after_switch')).toBeNull();
});

test('Claude relays are added and imported in the add dialog, and edited without retyping the token', async () => {
  const user = userEvent.setup();
  localStorage.setItem('cv_accounts_tab', 'claude');
  renderManager();
  await within(await screen.findByRole('region', { name: 'Current login' })).findByText('Relay One');
  await user.click(screen.getByRole('button', { name: 'Add account' }));
  expect(screen.getByText('Add Claude account')).toBeTruthy();
  expect(screen.getByRole('tab', { name: 'Official account', selected: true })).toBeTruthy();
  await user.click(screen.getByRole('tab', { name: 'Add relay' }));
  const save = screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  await user.type(screen.getByRole('textbox', { name: 'Name' }), 'Two');
  await user.type(screen.getByPlaceholderText('https://'), 'https://relay.two');
  await user.type(screen.getByLabelText('Token (ANTHROPIC_AUTH_TOKEN)'), 'sk-2');
  await user.click(save);
  expect(api.upsertClaudeProvider).toHaveBeenCalledWith({ id: null, name: 'Two', base_url: 'https://relay.two', token: 'sk-2', model: null });
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Saved the relay provider'));
  expect(screen.queryByText('Add Claude account')).toBeNull();

  await user.click(within(currentCard()).getByRole('button', { name: 'Edit' }));
  expect((screen.getByRole('textbox', { name: 'Model (optional, ANTHROPIC_MODEL)' }) as HTMLInputElement).value).toBe('m1');
  await user.click(screen.getByRole('button', { name: 'Save' }));
  expect(api.upsertClaudeProvider).toHaveBeenLastCalledWith({ id: 'cl-r', name: 'Relay One', base_url: 'https://relay.one', token: null, model: 'm1' });

  await user.click(screen.getByRole('button', { name: 'Add account' }));
  await user.click(screen.getByRole('tab', { name: 'Import relays from cc-switch' }));
  await user.click(screen.getByRole('button', { name: 'Import relays from cc-switch' }));
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Imported 2 relays from cc-switch'));
});

test('an unsaved current login can be saved, and a blocking note disables switching', async () => {
  const user = userEvent.setup();
  vi.mocked(api.listAccounts).mockImplementation(async (client) => ({
    ...STATES[client](),
    current: { email: 'new@example.com', saved: false },
    current_note: 'credentials are in the keyring',
  }));
  vi.mocked(api.saveCurrentAccount).mockResolvedValue('c');
  renderManager();
  expect(await screen.findByText('Not saved')).toBeTruthy();
  expect(screen.getByText('credentials are in the keyring')).toBeTruthy();
  expect((within(screen.getByRole('listitem', { name: 'personal@example.com' })).getByRole('button', { name: 'Switch' }) as HTMLButtonElement).disabled).toBe(true);
  await user.click(screen.getByRole('button', { name: 'Save current login' }));
  expect(api.saveCurrentAccount).toHaveBeenCalledWith('codex');
});

test('refreshing quota and deleting with confirmation', async () => {
  const user = userEvent.setup();
  vi.mocked(api.refreshAllAccountQuotas).mockResolvedValue([{ id: 'cx-b', error: 'x' }]);
  renderManager();
  const personal = await screen.findByRole('listitem', { name: 'personal@example.com' });
  await user.click(within(personal).getByRole('button', { name: 'Refresh quota' }));
  expect(api.refreshAccountQuota).toHaveBeenCalledWith('codex', 'cx-b');
  await user.click(screen.getByRole('button', { name: 'Refresh all quota' }));
  await waitFor(() => expect(toast).toHaveBeenCalledWith('warning', '1 accounts failed to refresh'));

  await user.click(within(screen.getByRole('listitem', { name: 'personal@example.com' })).getByRole('button', { name: 'Delete personal@example.com' }));
  expect(api.deleteAccount).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Delete' }));
  expect(api.deleteAccount).toHaveBeenCalledWith('codex', 'cx-b');
});

test('Codex accounts are added by signing in, from auth.json or from the current login', async () => {
  const user = userEvent.setup();
  let finish: (id: string) => void = () => {};
  vi.mocked(api.loginAccount).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  renderManager();
  await screen.findByRole('listitem', { name: 'personal@example.com' });
  await user.click(screen.getByRole('button', { name: 'Add account' }));
  expect(screen.getAllByRole('tab').slice(-3).map((t) => t.textContent)).toEqual(['Browser sign-in', 'Import auth.json', 'Current login']);
  await user.click(screen.getByRole('button', { name: 'Open browser to sign in' }));
  expect(api.loginAccount).toHaveBeenCalledWith('codex');
  expect(screen.getByText('Finish signing in in the browser window…')).toBeTruthy();
  finish('c');
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Signed in and saved the new account'));
  expect(screen.queryByText('Add Codex account')).toBeNull();

  vi.mocked(api.pickAuthJsonPath).mockResolvedValueOnce(null).mockResolvedValueOnce('C:\\backup\\auth.json');
  vi.mocked(api.importAccountFile).mockResolvedValue('c2');
  await user.click(screen.getByRole('button', { name: 'Add account' }));
  await user.click(screen.getByRole('tab', { name: 'Import auth.json' }));
  // Cancelling the file picker leaves the dialog as it was.
  await user.click(screen.getByRole('button', { name: 'Choose auth.json…' }));
  expect(api.importAccountFile).not.toHaveBeenCalled();
  expect(screen.getByText('Add Codex account')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: 'Choose auth.json…' }));
  expect(api.importAccountFile).toHaveBeenCalledWith('codex', 'C:\\backup\\auth.json');
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Imported the account'));

  await user.click(screen.getByRole('button', { name: 'Add account' }));
  await user.click(screen.getByRole('tab', { name: 'Current login' }));
  expect((screen.getByRole('button', { name: 'Already saved' }) as HTMLButtonElement).disabled).toBe(true);
});

test('browser sign-in shows the link to copy, and closing the dialog cancels it', async () => {
  const user = userEvent.setup();
  let emit: (url: string) => void = () => {};
  const stop = vi.fn();
  vi.mocked(api.onAccountLoginUrl).mockImplementation(async (callback) => {
    emit = callback;
    return stop;
  });
  let reject: (err: unknown) => void = () => {};
  vi.mocked(api.loginAccount).mockImplementation(() => new Promise((_, r) => { reject = r; }));
  localStorage.setItem('cv_accounts_tab', 'antigravity');
  renderManager();
  await screen.findByRole('listitem', { name: 'h@gmail.com' });
  await user.click(screen.getByRole('button', { name: 'Add account' }));
  expect(screen.getByText('Sign in with a Google account in the browser')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: 'Open browser to sign in' }));
  expect(api.loginAccount).toHaveBeenCalledWith('antigravity');
  act(() => emit('https://accounts.google.com/o/oauth2/v2/auth?state=s1'));
  expect(await screen.findByText('https://accounts.google.com/o/oauth2/v2/auth?state=s1')).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Copy link' })).toBeTruthy();
  // Other ways stay locked while the browser is open.
  await user.click(screen.getByRole('tab', { name: 'Paste tokens' }));
  expect(screen.queryByRole('textbox', { name: 'Paste tokens' })).toBeNull();

  await user.click(screen.getByTitle('关闭 (Esc)'));
  expect(api.cancelAccountLogin).toHaveBeenCalled();
  act(() => reject('sign-in cancelled'));
  await waitFor(() => expect(stop).toHaveBeenCalled());
  expect(toast).not.toHaveBeenCalledWith('error', expect.anything());
});

test('Antigravity accounts are added from pasted tokens, and failures stay in the dialog', async () => {
  const user = userEvent.setup();
  vi.mocked(api.importAccountTokens).mockResolvedValue({ imported: 1, failures: ['1//0gBad…: invalid_grant'] });
  localStorage.setItem('cv_accounts_tab', 'antigravity');
  renderManager();
  await screen.findByRole('listitem', { name: 'h@gmail.com' });
  const loads = vi.mocked(api.listAccounts).mock.calls.length;
  await user.click(screen.getByRole('button', { name: 'Add account' }));
  await user.click(screen.getByRole('tab', { name: 'Paste tokens' }));
  const submit = screen.getByRole('button', { name: 'Import' }) as HTMLButtonElement;
  expect(submit.disabled).toBe(true);
  await user.type(screen.getByRole('textbox', { name: 'Paste tokens' }), '1//0gGood{enter}1//0gBad');
  await user.click(submit);
  expect(api.importAccountTokens).toHaveBeenCalledWith('antigravity', '1//0gGood\n1//0gBad');
  expect(await screen.findByText('1//0gBad…: invalid_grant')).toBeTruthy();
  expect(toast).toHaveBeenCalledWith('success', 'Imported 1 accounts');
  expect(screen.getByRole('textbox', { name: 'Paste tokens' })).toBeTruthy();
  await waitFor(() => expect(vi.mocked(api.listAccounts).mock.calls.length).toBeGreaterThan(loads));
});
