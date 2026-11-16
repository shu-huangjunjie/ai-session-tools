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
    pickSavePath: vi.fn(),
    pickFolderPath: vi.fn(),
    openExternalUrl: vi.fn(),
    loginAccount: vi.fn(),
    submitAccountLoginCallback: vi.fn(),
    startCodexDeviceLogin: vi.fn(),
    completeCodexDeviceLogin: vi.fn(),
    cancelAccountLogin: vi.fn(),
    onAccountLoginUrl: vi.fn(),
    importAccountTokens: vi.fn(),
    switchAccount: vi.fn(),
    deleteAccount: vi.fn(),
    updateAccountMeta: vi.fn(),
    batchUpdateAccountTags: vi.fn(),
    batchDeleteAccounts: vi.fn(),
    refreshAccountQuota: vi.fn(),
    refreshAllAccountQuotas: vi.fn(),
    upsertClaudeProvider: vi.fn(),
    upsertCodexProvider: vi.fn(),
    testProviderConnection: vi.fn(),
    importCcSwitchProviders: vi.fn(),
    importCcSwitchClaudeProviders: vi.fn(),
    importCockpitToolsAccounts: vi.fn(),
    exportAccountsJson: vi.fn(),
    launchAccountCli: vi.fn(),
    launchAccountTarget: vi.fn(),
  },
  errorMessage: (e: unknown) => String(e),
}));

const inHours = (h: number) => new Date(Date.now() + h * 3600000).toISOString();
const account = (id: string, extra: Partial<AccountView> = {}): AccountView => ({
  id,
  is_current: false,
  has_quota: true,
  tags: [],
  added_at: '2026-10-01T00:00:00Z',
  ...extra,
});

const STATES: Record<AccountClient, () => AccountsState> = {
  codex: () => ({
    auth_path: 'C:\\Users\\Demo\\.codex\\auth.json',
    current: { email: 'work@example.com', plan_type: 'pro', saved: true },
    notes: [],
    cockpit_import_count: 0,
    cc_switch_available: true,
    accounts: [
      account('cx-a', {
        email: 'work@example.com',
        plan_type: 'pro',
        is_current: true,
        tags: ['primary'],
        note: 'Main dev account',
        added_at: '2026-10-02T00:00:00Z',
        quota: {
          plan_type: 'pro',
          windows: [
            { kind: 'primary', used_percent: 38, window_minutes: 300, resets_at: inHours(2.5) },
            { kind: 'secondary', used_percent: 93, window_minutes: 10080, resets_at: inHours(72) },
          ],
        },
      }),
      account('cx-b', {
        email: 'personal@example.com',
        plan_type: 'free',
        tags: ['backup'],
        added_at: '2026-10-01T00:00:00Z',
        quota_error: 'token expired, sign in again',
        verification_url: 'https://accounts.google.com/verify',
      }),
    ],
  }),
  claude: () => ({
    auth_path: 'C:\\Users\\Demo\\.claude',
    current: { kind: 'provider', name: 'Relay One', saved: true },
    notes: ['a relay overrides the official login'],
    cockpit_import_count: 0,
    cc_switch_available: true,
    accounts: [
      account('cl-o', {
        kind: 'oauth',
        email: 'me@example.com',
        plan_type: 'max',
        quota: {
          windows: [{ kind: 'seven_day_opus', label: 'Opus', used_percent: 55, window_minutes: 10080 }],
        },
      }),
      account('cl-r', {
        kind: 'provider',
        name: 'Relay One',
        detail: 'https://relay.one · m1',
        is_current: true,
        has_quota: false,
      }),
      account('cl-r2', {
        kind: 'provider',
        name: 'Relay Two',
        detail: 'https://relay.two',
        has_quota: false,
      }),
    ],
  }),
  antigravity: () => ({
    auth_path: 'C:\\state.vscdb',
    current: { email: 'g@gmail.com', saved: true },
    notes: [],
    cockpit_import_count: 0,
    cc_switch_available: false,
    accounts: [
      account('ag-1', {
        email: 'g@gmail.com',
        is_current: true,
        quota: {
          credits_balance: '1200 credits',
          windows: [{ kind: 'gemini', label: 'Gemini 3 Pro', used_percent: 80 }],
        },
      }),
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
  vi.mocked(api.updateAccountMeta).mockResolvedValue(undefined);
  vi.mocked(api.batchUpdateAccountTags).mockResolvedValue(2);
  vi.mocked(api.batchDeleteAccounts).mockResolvedValue(2);
  vi.mocked(api.upsertClaudeProvider).mockResolvedValue('new');
  vi.mocked(api.upsertCodexProvider).mockResolvedValue('new-cx');
  vi.mocked(api.testProviderConnection).mockResolvedValue({
    ok: true,
    latency_ms: 88,
    models: ['claude-sonnet-4-6', 'claude-opus-4-6'],
    message: 'HTTP 200 OK',
  });
  vi.mocked(api.importCcSwitchProviders).mockResolvedValue(2);
  vi.mocked(api.importCcSwitchClaudeProviders).mockResolvedValue(2);
  vi.mocked(api.importCockpitToolsAccounts).mockResolvedValue({ imported: 1, failures: [] });
  vi.mocked(api.exportAccountsJson).mockResolvedValue('[]');
  vi.mocked(api.launchAccountCli).mockResolvedValue(undefined);
  vi.mocked(api.pickSavePath).mockResolvedValue('C:\\export\\codex-accounts.json');
  vi.mocked(api.pickFolderPath).mockResolvedValue('D:\\workspace');
  vi.mocked(api.openExternalUrl).mockResolvedValue(undefined);
  vi.mocked(api.submitAccountLoginCallback).mockResolvedValue(undefined);
  vi.mocked(api.onAccountLoginUrl).mockResolvedValue(() => {});
  vi.mocked(api.cancelAccountLogin).mockResolvedValue(undefined);
});

afterEach(cleanup);

test('unified view lists current and other accounts together with remaining-percent quota bars', async () => {
  renderManager();
  const currentItem = await screen.findByRole('listitem', { name: 'work@example.com' });
  const personal = await screen.findByRole('listitem', { name: 'personal@example.com' });
  expect(api.listAccounts).toHaveBeenCalledWith('codex');

  expect(within(currentItem).getByText('Current')).toBeTruthy();
  expect(within(currentItem).getByText('5 hours')).toBeTruthy();
  expect(within(currentItem).getByText('62% left')).toBeTruthy();
  expect(within(currentItem).getByText('7% left')).toBeTruthy();
  expect(
    within(currentItem)
      .getAllByRole('progressbar')
      .map((b) => b.getAttribute('aria-valuenow'))
  ).toEqual(['62', '7']);
  expect(within(currentItem).getByText(/resets in 2\.5 h/)).toBeTruthy();
  expect(within(currentItem).getByText('#primary')).toBeTruthy();
  expect(within(currentItem).getByText('Main dev account')).toBeTruthy();
  expect(within(currentItem).queryByRole('button', { name: 'Switch' })).toBeNull();

  expect(within(personal).getByText('token expired, sign in again')).toBeTruthy();
  expect(within(personal).getByRole('button', { name: 'Verify' })).toBeTruthy();
  expect(screen.queryByRole('region', { name: 'Current login' })).toBeNull();
});

test('each tab loads and switches only its own client', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(
    within(await screen.findByRole('listitem', { name: 'personal@example.com' })).getByRole('button', {
      name: 'Switch',
    })
  );
  expect(api.switchAccount).toHaveBeenCalledWith('codex', 'cx-b', true);

  await user.click(screen.getByRole('tab', { name: 'Claude' }));
  const oauth = await screen.findByRole('listitem', { name: 'me@example.com' });
  expect(api.listAccounts).toHaveBeenCalledWith('claude');
  expect(screen.queryByText('work@example.com')).toBeNull();
  expect(screen.getByText('a relay overrides the official login')).toBeTruthy();
  expect(within(oauth).getByText('Weekly · Opus')).toBeTruthy();
  expect(screen.queryByRole('switch')).toBeNull();

  await user.click(within(oauth).getByRole('button', { name: 'Switch' }));
  expect(api.switchAccount).toHaveBeenLastCalledWith('claude', 'cl-o', false);

  const relay = screen.getByRole('listitem', { name: 'Relay One' });
  expect(within(relay).getByText('Current')).toBeTruthy();
  expect(within(relay).getByText('Relays have no common quota API')).toBeTruthy();
  expect(within(relay).queryByRole('button', { name: /Refresh quota/ })).toBeNull();

  await user.click(screen.getByRole('tab', { name: 'Antigravity' }));
  const agCurrent = await screen.findByRole('listitem', { name: 'g@gmail.com' });
  const pro = within(agCurrent).getByRole('progressbar', { name: 'Gemini Pro' });
  expect([pro.getAttribute('aria-valuenow'), pro.getAttribute('aria-valuetext')]).toEqual([
    '20',
    '20% left',
  ]);
  expect(within(agCurrent).getByText('Extra usage: 1200 credits')).toBeTruthy();
  expect(within(agCurrent).queryByText('3 Pro')).toBeNull();
  await user.click(within(agCurrent).getByRole('button', { name: 'All 1 models' }));
  expect(within(agCurrent).getByText('3 Pro')).toBeTruthy();

  await user.click(
    screen.getByRole('switch', { name: 'Close and reopen Antigravity when switching' })
  );
  await user.click(
    within(screen.getByRole('listitem', { name: 'h@gmail.com' })).getByRole('button', {
      name: 'Switch',
    })
  );
  expect(api.switchAccount).toHaveBeenLastCalledWith('antigravity', 'ag-2', false);
  expect(localStorage.getItem('cv_antigravity_restart_after_switch')).toBe('false');
  expect(localStorage.getItem('cv_codex_restart_after_switch')).toBeNull();
});

test('Claude providers support connection tests, model chips, advanced mapping, editing and cc-switch import', async () => {
  const user = userEvent.setup();
  localStorage.setItem('cv_accounts_tab', 'claude');
  renderManager();
  await screen.findByRole('listitem', { name: 'Relay One' });

  await user.click(screen.getByRole('button', { name: 'Add account' }));
  expect(screen.getByText('Add Claude account')).toBeTruthy();
  expect(screen.getByRole('tab', { name: 'OAuth Sign-in', selected: true })).toBeTruthy();

  await user.click(screen.getByRole('tab', { name: 'API Key / Provider' }));
  const save = screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);

  await user.type(screen.getByRole('textbox', { name: 'Name' }), 'Two');
  await user.type(screen.getByPlaceholderText('https://'), 'https://relay.two');
  await user.type(screen.getByLabelText('Token (ANTHROPIC_AUTH_TOKEN)'), 'sk-2');

  await user.click(screen.getByRole('button', { name: 'Test connection' }));
  expect(api.testProviderConnection).toHaveBeenCalledWith(
    'https://relay.two',
    'sk-2',
    'ANTHROPIC_AUTH_TOKEN'
  );
  await user.click(await screen.findByRole('button', { name: 'claude-sonnet-4-6' }));

  await user.click(screen.getByRole('button', { name: 'Advanced model mapping (Haiku / Sonnet / Opus)' }));
  await user.type(screen.getByLabelText('Haiku (ANTHROPIC_DEFAULT_HAIKU_MODEL)'), 'claude-haiku-4-5');

  await user.click(save);
  expect(api.upsertClaudeProvider).toHaveBeenCalledWith({
    id: null,
    name: 'Two',
    base_url: 'https://relay.two',
    token: 'sk-2',
    model: 'claude-sonnet-4-6',
    auth_field: 'ANTHROPIC_AUTH_TOKEN',
    haiku_model: 'claude-haiku-4-5',
    sonnet_model: null,
    opus_model: null,
  });
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Saved the relay provider'));

  const relayOne = await screen.findByRole('listitem', { name: 'Relay One' });
  await user.click(within(relayOne).getByRole('button', { name: 'Edit Relay One' }));
  expect(
    (screen.getByRole('textbox', { name: 'Model (optional, ANTHROPIC_MODEL)' }) as HTMLInputElement)
      .value
  ).toBe('m1');
  await user.click(screen.getByRole('button', { name: 'Save' }));
  expect(api.upsertClaudeProvider).toHaveBeenLastCalledWith({
    id: 'cl-r',
    name: 'Relay One',
    base_url: 'https://relay.one',
    token: null,
    model: 'm1',
    auth_field: 'ANTHROPIC_AUTH_TOKEN',
    haiku_model: null,
    sonnet_model: null,
    opus_model: null,
  });

  await user.click(screen.getByRole('button', { name: 'Add account' }));
  await user.click(screen.getByRole('tab', { name: 'Local & External' }));
  await user.click(screen.getByRole('button', { name: 'Import relays from cc-switch' }));
  expect(api.importCcSwitchProviders).toHaveBeenCalledWith('claude');
  await waitFor(() =>
    expect(toast).toHaveBeenCalledWith('success', 'Imported 2 providers from cc-switch')
  );
});

test('an unsaved current login shows callout banner and blocking note disables switching', async () => {
  const user = userEvent.setup();
  vi.mocked(api.listAccounts).mockImplementation(async (client) => ({
    ...STATES[client](),
    current: { email: 'new@example.com', saved: false },
    current_note: 'credentials are in the keyring',
  }));
  vi.mocked(api.saveCurrentAccount).mockResolvedValue('c');
  renderManager();
  expect(await screen.findByText('Not saved')).toBeTruthy();
  expect(screen.getByRole('region', { name: 'Current login' })).toBeTruthy();
  expect(screen.getByText('credentials are in the keyring')).toBeTruthy();
  expect(
    (
      within(screen.getByRole('listitem', { name: 'personal@example.com' })).getByRole('button', {
        name: 'Switch',
      }) as HTMLButtonElement
    ).disabled
  ).toBe(true);
  await user.click(screen.getByRole('button', { name: 'Save current login' }));
  expect(api.saveCurrentAccount).toHaveBeenCalledWith('codex');
});

test('refreshing quota and single-account deleting with confirmation', async () => {
  const user = userEvent.setup();
  vi.mocked(api.refreshAllAccountQuotas).mockResolvedValue([{ id: 'cx-b', error: 'x' }]);
  renderManager();
  const personal = await screen.findByRole('listitem', { name: 'personal@example.com' });
  await user.click(within(personal).getByRole('button', { name: /Refresh quota/ }));
  expect(api.refreshAccountQuota).toHaveBeenCalledWith('codex', 'cx-b');

  await user.click(screen.getByRole('button', { name: 'Refresh all quota' }));
  await waitFor(() =>
    expect(toast).toHaveBeenCalledWith('warning', '1 accounts failed to refresh')
  );

  await user.click(
    within(screen.getByRole('listitem', { name: 'personal@example.com' })).getByRole('button', {
      name: 'Delete personal@example.com',
    })
  );
  expect(api.deleteAccount).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Delete' }));
  expect(api.deleteAccount).toHaveBeenCalledWith('codex', 'cx-b');
});

test('Codex accounts support PKCE browser login, Device Code, file import, and current login tab', async () => {
  const user = userEvent.setup();
  let finish: (id: string) => void = () => {};
  vi.mocked(api.loginAccount).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      })
  );
  renderManager();
  await screen.findByRole('listitem', { name: 'personal@example.com' });

  await user.click(screen.getByRole('button', { name: 'Add account' }));
  expect(screen.getAllByRole('tab').slice(-4).map((t) => t.textContent)).toEqual([
    'OAuth Sign-in',
    'Token / JSON',
    'API Key / Provider',
    'Local & External',
  ]);
  await user.click(screen.getByRole('button', { name: 'Open browser to sign in' }));
  expect(api.loginAccount).toHaveBeenCalledWith('codex', 'oauth');
  expect(screen.getByText('Finish signing in in the browser window…')).toBeTruthy();
  finish('c');
  await waitFor(() =>
    expect(toast).toHaveBeenCalledWith('success', 'Signed in and saved the new account')
  );
  expect(screen.queryByText('Add Codex account')).toBeNull();

  // Codex Device Code 2-step flow
  let finishDevice: (id: string) => void = () => {};
  vi.mocked(api.startCodexDeviceLogin).mockResolvedValue({
    device_auth_id: 'dev-1',
    user_code: 'ABCD-1234',
    verification_url: 'https://auth.openai.com/device',
    interval_secs: 5,
  });
  vi.mocked(api.completeCodexDeviceLogin).mockImplementation(
    () =>
      new Promise((resolve) => {
        finishDevice = resolve;
      })
  );
  await user.click(screen.getByRole('button', { name: 'Add account' }));
  await user.click(screen.getByRole('button', { name: 'Device Code' }));
  await user.click(screen.getByRole('button', { name: 'Get device code' }));
  expect(await screen.findByText('ABCD-1234')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: 'Open verification page' }));
  expect(api.openExternalUrl).toHaveBeenCalledWith('https://auth.openai.com/device');
  finishDevice('cx-device');
  await waitFor(() =>
    expect(toast).toHaveBeenCalledWith('success', 'Signed in and saved the new account')
  );

  // File import in Token / JSON tab
  vi.mocked(api.pickAuthJsonPath)
    .mockResolvedValueOnce(null)
    .mockResolvedValueOnce('C:\\backup\\auth.json');
  vi.mocked(api.importAccountFile).mockResolvedValue({ imported: 1, failures: [] });
  await user.click(screen.getByRole('button', { name: 'Add account' }));
  await user.click(screen.getByRole('tab', { name: 'Token / JSON' }));
  await user.click(screen.getByRole('button', { name: 'Import JSON / auth.json file…' }));
  expect(api.importAccountFile).not.toHaveBeenCalled();
  expect(screen.getByText('Add Codex account')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: 'Import JSON / auth.json file…' }));
  expect(api.importAccountFile).toHaveBeenCalledWith('codex', 'C:\\backup\\auth.json');
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Imported 1 accounts'));

  // Local & External tab shows Already saved for active login
  await user.click(screen.getByRole('button', { name: 'Add account' }));
  await user.click(screen.getByRole('tab', { name: 'Local & External' }));
  expect((screen.getByRole('button', { name: 'Already saved' }) as HTMLButtonElement).disabled).toBe(
    true
  );
});

test('browser sign-in supports copying link, submitting manual callback URL, and cancelling on close', async () => {
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
  expect(api.loginAccount).toHaveBeenCalledWith('antigravity', undefined);

  act(() => emit('https://accounts.google.com/o/oauth2/v2/auth?state=s1'));
  expect(await screen.findByText('https://accounts.google.com/o/oauth2/v2/auth?state=s1')).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Copy link' })).toBeTruthy();

  await user.type(
    screen.getByPlaceholderText('http://localhost:.../callback?code=... or code'),
    'http://localhost:8888/callback?code=c123'
  );
  await user.click(screen.getByRole('button', { name: 'Submit callback' }));
  expect(api.submitAccountLoginCallback).toHaveBeenCalledWith(
    'http://localhost:8888/callback?code=c123'
  );

  await user.click(screen.getByRole('tab', { name: 'Token / JSON' }));
  expect(screen.queryByRole('textbox', { name: 'Paste tokens' })).toBeNull();

  await user.click(screen.getByTitle('关闭 (Esc)'));
  expect(api.cancelAccountLogin).toHaveBeenCalled();
  act(() => reject('sign-in cancelled'));
  await waitFor(() => expect(stop).toHaveBeenCalled());
  expect(toast).not.toHaveBeenCalledWith('error', expect.anything());
});

test('Antigravity accounts are added from pasted tokens, and failures stay in the dialog', async () => {
  const user = userEvent.setup();
  vi.mocked(api.importAccountTokens).mockResolvedValue({
    imported: 1,
    failures: ['1//0gBad…: invalid_grant'],
  });
  localStorage.setItem('cv_accounts_tab', 'antigravity');
  renderManager();
  await screen.findByRole('listitem', { name: 'h@gmail.com' });
  const loads = vi.mocked(api.listAccounts).mock.calls.length;
  await user.click(screen.getByRole('button', { name: 'Add account' }));
  await user.click(screen.getByRole('tab', { name: 'Token / JSON' }));
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

test('view modes, search, filters, sorting and privacy mode work cleanly', async () => {
  const user = userEvent.setup();
  renderManager();
  await screen.findByRole('listitem', { name: 'work@example.com' });

  await user.click(screen.getByRole('button', { name: 'Compact' }));
  expect(localStorage.getItem('ai-session-tools.accounts.viewMode')).toBe('compact');
  await user.click(screen.getByRole('button', { name: 'List' }));
  expect(localStorage.getItem('ai-session-tools.accounts.viewMode')).toBe('list');
  await user.click(screen.getByRole('button', { name: 'Grid' }));

  const search = screen.getByRole('textbox', { name: 'Search email, name, tag or note…' });
  await user.type(search, 'personal');
  expect(screen.queryByRole('listitem', { name: 'work@example.com' })).toBeNull();
  expect(screen.getByRole('listitem', { name: 'personal@example.com' })).toBeTruthy();
  await user.clear(search);

  await user.selectOptions(screen.getByLabelText('Status'), 'error');
  expect(screen.queryByRole('listitem', { name: 'work@example.com' })).toBeNull();
  expect(screen.getByRole('listitem', { name: 'personal@example.com' })).toBeTruthy();
  await user.selectOptions(screen.getByLabelText('Status'), 'all');

  await user.selectOptions(screen.getByLabelText('Sort'), 'name');
  expect(screen.getAllByRole('listitem').map((el) => el.getAttribute('aria-label'))).toEqual([
    'work@example.com',
    'personal@example.com',
  ]);
  await user.click(screen.getByRole('button', { name: 'Descending' }));
  expect(screen.getAllByRole('listitem').map((el) => el.getAttribute('aria-label'))).toEqual([
    'personal@example.com',
    'work@example.com',
  ]);

  await user.click(screen.getByRole('button', { name: 'Privacy mask' }));
  expect(await screen.findByRole('listitem', { name: 'wo***@example.com' })).toBeTruthy();
  expect(screen.getByRole('listitem', { name: 'pe***@example.com' })).toBeTruthy();
});

test('tags & note modal, note clearing, batch actions, Cockpit Tools banner, export, verify & CLI launch', async () => {
  const user = userEvent.setup();
  vi.mocked(api.listAccounts).mockImplementation(async (client) => ({
    ...STATES[client](),
    cockpit_import_count: 3,
  }));
  renderManager();

  // Cockpit Tools auto-discovery banner
  expect(await screen.findByText('Detected 3 Codex accounts in local Cockpit Tools')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: 'Import Cockpit Tools' }));
  expect(api.importCockpitToolsAccounts).toHaveBeenCalledWith('codex');
  await waitFor(() =>
    expect(toast).toHaveBeenCalledWith('success', 'Imported 1 accounts from Cockpit Tools')
  );

  // Export JSON from header
  await user.click(screen.getByRole('button', { name: 'Export JSON' }));
  expect(api.pickSavePath).toHaveBeenCalledWith('codex-accounts.json', 'JSON', 'json');
  expect(api.exportAccountsJson).toHaveBeenCalledWith(
    'codex',
    undefined,
    'C:\\export\\codex-accounts.json'
  );

  // Verification URL & Launch CLI actions on card
  const personal = await screen.findByRole('listitem', { name: 'personal@example.com' });
  await user.click(within(personal).getByRole('button', { name: 'Verify' }));
  expect(api.openExternalUrl).toHaveBeenCalledWith('https://accounts.google.com/verify');

  await user.click(
    within(personal).getByRole('button', { name: 'Quick Launch personal@example.com' })
  );
  expect(await screen.findByRole('heading', { name: 'Switch & Launch' })).toBeTruthy();

  await user.click(screen.getByRole('button', { name: /Codex CLI/ }));
  await user.click(screen.getByRole('button', { name: 'Choose folder' }));
  await user.type(
    screen.getByLabelText('Launch Command / Extra Flags (Optional)'),
    '--yolo'
  );
  await user.click(screen.getByRole('button', { name: 'Switch & Run' }));

  expect(api.pickFolderPath).toHaveBeenCalled();
  expect(api.launchAccountTarget).toHaveBeenCalledWith({
    client: 'codex',
    accountId: 'cx-b',
    toolId: 'codex_cli',
    folderPath: 'D:\\workspace',
    terminal: 'system',
    customCommand: '--yolo',
  });

  // Single-account tags & note modal (update and clear note with "")
  await user.click(
    within(personal).getByRole('button', { name: 'Tags & note personal@example.com' })
  );
  await user.type(screen.getByLabelText('Tags'), 'sprint{enter}');
  await user.type(screen.getByLabelText('Note'), 'Team fallback');
  await user.click(screen.getByRole('button', { name: 'Save' }));
  expect(api.updateAccountMeta).toHaveBeenCalledWith(
    'codex',
    'cx-b',
    ['backup', 'sprint'],
    'Team fallback'
  );

  // Verify clearing note passes "" instead of null
  await user.click(
    within(screen.getByRole('listitem', { name: 'work@example.com' })).getByRole('button', {
      name: 'Tags & note work@example.com',
    })
  );
  const noteInput = screen.getByLabelText('Note');
  await user.clear(noteInput);
  await user.click(screen.getByRole('button', { name: 'Save' }));
  expect(api.updateAccountMeta).toHaveBeenLastCalledWith('codex', 'cx-a', ['primary'], '');

  // Batch selection: tags, refresh, export, delete
  await user.click(screen.getByLabelText('Select all'));
  expect(screen.getByText('2 selected')).toBeTruthy();

  await user.click(screen.getByRole('button', { name: 'Tags' }));
  await user.type(screen.getByLabelText('Add tags (comma or space separated)'), 'vip');
  await user.click(screen.getByRole('button', { name: 'Save' }));
  expect(api.batchUpdateAccountTags).toHaveBeenCalledWith('codex', ['cx-a', 'cx-b'], ['vip'], []);

  await user.click(screen.getByRole('button', { name: 'Refresh' }));
  expect(api.refreshAllAccountQuotas).toHaveBeenCalledWith('codex', ['cx-a', 'cx-b']);

  await user.click(screen.getByRole('button', { name: 'Export' }));
  expect(api.exportAccountsJson).toHaveBeenLastCalledWith(
    'codex',
    ['cx-a', 'cx-b'],
    'C:\\export\\codex-accounts.json'
  );

  await user.click(screen.getByRole('button', { name: 'Delete' }));
  await user.click(screen.getAllByRole('button', { name: 'Delete' })[1]);
  expect(api.batchDeleteAccounts).toHaveBeenCalledWith('codex', ['cx-a', 'cx-b']);
});
