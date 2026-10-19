import { useEffect, useRef, useState } from 'react';
import * as Popover from '@radix-ui/react-popover';
import { Plus } from 'lucide-react';
import { calendarClient, desktop, errorMessage } from './client';
import type { ProviderDescriptor } from './generated/ProviderDescriptor';
import type { AccountSummary } from './generated/AccountSummary';
import type { CalendarDiscovery } from './generated/CalendarDiscovery';
import type { DisconnectReview } from './generated/DisconnectReview';
import { SidebarPopup } from './SidebarPopup';

// The browser preview is an explicit harness and cannot authenticate a real account.
const previewProviders: ProviderDescriptor[] = [
  {
    id: 'local',
    name: 'On this device',
    connectionAvailable: true,
    unavailableReason: null,
    capabilities: {
      read: true,
      create: true,
      edit: true,
      delete: true,
      recurrence: true,
      customizeMetadata: true,
      moveEvent: true,
    },
  },
  {
    id: 'google',
    name: 'Google Calendar',
    connectionAvailable: false,
    unavailableReason: 'Connect Google accounts in the desktop app.',
    capabilities: {
      read: true,
      create: false,
      edit: false,
      delete: false,
      recurrence: false,
      customizeMetadata: true,
      moveEvent: false,
    },
  },
];

export function ProviderControls({
  busy,
  accounts,
  onLocal,
  onRefresh,
}: {
  busy: boolean;
  accounts: AccountSummary[];
  onLocal: (anchor: HTMLElement) => void;
  onRefresh: () => void;
}) {
  const [providers, setProviders] = useState<ProviderDescriptor[]>(desktop ? [] : previewProviders);
  const [failure, setFailure] = useState('');
  const [open, setOpen] = useState(false);
  const [connecting, setConnecting] = useState<{
    provider: ProviderDescriptor;
    account?: AccountSummary;
  } | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const plus = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const transferring = useRef(false);
  const load = () => {
    if (desktop)
      void calendarClient
        .providers()
        .then((values) => {
          setProviders(values);
          setFailure('');
        })
        .catch((e) => setFailure(errorMessage(e)));
  };
  useEffect(load, []);
  useEffect(() => {
    // Descriptors can arrive after the popup's initial autofocus callback.
    if (
      open &&
      (document.activeElement === plus.current || document.activeElement === document.body)
    ) {
      menu.current
        ?.querySelector<HTMLButtonElement>('button[role="menuitem"]:not(:disabled)')
        ?.focus();
    }
  }, [open, providers]);
  const items = () =>
    Array.from(
      menu.current?.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]:not(:disabled)') ??
        [],
    );
  return (
    <>
      <Popover.Root
        open={open}
        onOpenChange={(value) => {
          setOpen(value);
          if (value) load();
        }}
      >
        <Popover.Trigger asChild>
          <button
            ref={plus}
            className="icon-button"
            aria-label="Create calendar"
            aria-haspopup="menu"
            disabled={busy}
            onKeyDown={(e) => {
              if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
                e.preventDefault();
                setOpen(true);
                load();
              }
            }}
          >
            <Plus size={15} />
          </button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content
            ref={menu}
            className="calendar-menu provider-menu"
            role="menu"
            aria-label="Calendar providers"
            side="right"
            align="start"
            sideOffset={6}
            onOpenAutoFocus={(e) => {
              e.preventDefault();
              transferring.current = false;
              items()[0]?.focus();
            }}
            onCloseAutoFocus={(e) => {
              if (transferring.current) e.preventDefault();
            }}
            onKeyDown={(e) => {
              const buttons = items();
              const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
              if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
                e.preventDefault();
                buttons[
                  e.key === 'Home'
                    ? 0
                    : e.key === 'End'
                      ? buttons.length - 1
                      : (index + (e.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length
                ]?.focus();
              }
              if (e.key === 'Tab') {
                e.preventDefault();
                setOpen(false);
              }
            }}
          >
            {providers.map((provider) => (
              <div key={provider.id}>
                <button
                  role="menuitem"
                  disabled={!provider.connectionAvailable}
                  aria-describedby={
                    provider.unavailableReason ? `provider-${provider.id}-reason` : undefined
                  }
                  onClick={() => {
                    transferring.current = true;
                    opener.current = plus.current;
                    setOpen(false);
                    if (provider.id === 'local') onLocal(plus.current!);
                    else setConnecting({ provider });
                  }}
                >
                  {provider.name}
                </button>
                {provider.unavailableReason && (
                  <p className="provider-reason" id={`provider-${provider.id}-reason`}>
                    {provider.unavailableReason}
                  </p>
                )}
              </div>
            ))}
            {failure && (
              <div>
                <p role="alert" className="provider-reason">
                  {failure}
                </p>
                <button role="menuitem" onClick={load}>
                  Retry providers
                </button>
              </div>
            )}
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
      {accounts.length > 0 && (
        <div className="connected-accounts" aria-label="Connected accounts">
          {accounts.map((account) => (
            <button
              key={account.id}
              className="text-button"
              disabled={busy}
              onClick={(e) => {
                opener.current = e.currentTarget;
                const provider = providers.find((p) => p.id === account.providerId);
                if (provider) setConnecting({ provider, account });
                else {
                  setFailure('This account provider is unavailable.');
                  setOpen(true);
                }
              }}
            >
              Manage {account.displayName} ·{' '}
              {account.connectionState === 'connected' ? 'Connected' : 'Reconnect'}
            </button>
          ))}
        </div>
      )}
      {connecting && (
        <ConnectionDialog
          key={`${connecting.provider.id}-${connecting.account?.id ?? 'new'}`}
          provider={connecting.provider}
          account={connecting.account}
          opener={opener}
          onClose={() => setConnecting(null)}
          onRefresh={onRefresh}
        />
      )}
    </>
  );
}

function ConnectionDialog({
  provider,
  account,
  opener,
  onClose,
  onRefresh,
}: {
  provider: ProviderDescriptor;
  account?: AccountSummary;
  opener: React.RefObject<HTMLElement | null>;
  onClose: () => void;
  onRefresh: () => void;
}) {
  const [status, setStatus] = useState('');
  const [failure, setFailure] = useState('');
  const [discovery, setDiscovery] = useState<CalendarDiscovery | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [working, setWorking] = useState(false);
  const [closing, setClosing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [disconnectReview, setDisconnectReview] = useState<DisconnectReview | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const session = useRef<string | null>(null);
  const accountId = useRef(account?.id);
  const generation = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const discover = async (id: string, version: number) => {
    setStatus('Discovering calendars…');
    setWorking(true);
    setFailure('');
    try {
      const value = await calendarClient.discoverCalendars(id);
      if (version !== generation.current) return;
      setDiscovery(value);
      setSelected(value.calendars.filter((c) => c.selected).map((c) => c.remoteId));
      setStatus('');
    } catch (e) {
      if (version === generation.current) {
        setFailure(errorMessage(e));
        setStatus('');
        onRefresh();
      }
    } finally {
      if (version === generation.current) setWorking(false);
    }
  };
  const start = async () => {
    const version = ++generation.current;
    setDiscovery(null);
    setWorking(true);
    setFailure('');
    setStatus('Opening your browser…');
    try {
      if (session.current) {
        await calendarClient.cancelConnection(provider.id, session.current);
        session.current = null;
      }
      if (version !== generation.current) return;
      const value = await calendarClient.startConnection(provider.id);
      if (version !== generation.current) {
        await calendarClient.cancelConnection(provider.id, value.id);
        return;
      }
      session.current = value.id;
      setStatus('Complete authorization in your browser.');
      const poll = async () => {
        try {
          const current = await calendarClient.connectionStatus(provider.id, value.id);
          if (version !== generation.current) return;
          if (current.progress === 'connected' && current.accountId) {
            session.current = null;
            accountId.current = current.accountId;
            onRefresh();
            await discover(current.accountId, version);
          } else if (current.progress === 'waitingForBrowser')
            timer.current = setTimeout(() => void poll(), 500);
          else {
            session.current = null;
            setWorking(false);
            setStatus('');
            setFailure(current.failure?.message ?? 'Connection cancelled.');
          }
        } catch (e) {
          if (version === generation.current) {
            setFailure(errorMessage(e));
            setStatus('');
            setWorking(false);
          }
        }
      };
      void poll();
    } catch (e) {
      if (version === generation.current) {
        setFailure(errorMessage(e));
        setWorking(false);
        setStatus('');
      }
    }
  };
  const close = async (completed = false) => {
    if (closing || (saving && !completed)) return;
    setClosing(true);
    try {
      if (session.current) await calendarClient.cancelConnection(provider.id, session.current);
      generation.current++;
      if (timer.current) clearTimeout(timer.current);
      session.current = null;
      onClose();
    } catch (e) {
      setFailure(errorMessage(e));
      setClosing(false);
    }
  };
  useEffect(() => {
    // Existing accounts can load their checklist; new connections require an explicit click.
    const kickoff = setTimeout(() => {
      if (account?.connectionState === 'connected') void discover(account.id, generation.current);
    }, 0);
    return () => {
      clearTimeout(kickoff);
      generation.current++;
      if (timer.current) clearTimeout(timer.current);
      if (session.current)
        void calendarClient.cancelConnection(provider.id, session.current).catch(() => undefined);
    };
  }, []);
  return (
    <SidebarPopup
      open
      onOpenChange={(value) => {
        if (!value) void close();
      }}
      title={account ? 'Manage connected account' : `Connect ${provider.name}`}
      closeLabel="Close connection"
      opener={opener}
    >
      <div className="connection-content">
        <p>{discovery?.account.displayName ?? account?.displayName ?? provider.name}</p>
        <p className="provider-reason">
          Selected calendars synchronize while aika is open. Cached events remain available offline.
          Ordinary events in writable calendars support editing.
        </p>
        {status && <p role="status">{status}</p>}
        {failure && (
          <p role="alert" className="editor-error">
            {failure}
          </p>
        )}
        {discovery && !disconnectReview && (
          <fieldset disabled={working || closing} className="import-checklist">
            <legend>Calendars to include</legend>
            {discovery.calendars.length === 0 && <p>No calendars were found.</p>}
            {discovery.calendars.map((c, index) => (
              <div key={c.remoteId}>
                <label>
                  <input
                    type="checkbox"
                    checked={selected.includes(c.remoteId)}
                    disabled={!c.eligible}
                    aria-describedby={`calendar-access-${index}`}
                    onChange={(e) =>
                      setSelected((values) =>
                        e.target.checked
                          ? [...values, c.remoteId]
                          : values.filter((v) => v !== c.remoteId),
                      )
                    }
                  />
                  <span
                    className="calendar-dot"
                    style={{ background: c.color }}
                    aria-hidden="true"
                  />
                  {c.name}
                </label>
                <p id={`calendar-access-${index}`} className="provider-reason">
                  {c.disabledReason ?? `Access: ${c.accessRole}`}
                </p>
              </div>
            ))}
          </fieldset>
        )}
        <div className="calendar-dialog-actions">
          <button className="text-button" disabled={closing || saving} onClick={() => void close()}>
            Cancel
          </button>
          {!working && !discovery && !disconnectReview && (
            <button
              className="primary-button"
              disabled={!provider.connectionAvailable || closing}
              onClick={() => void start()}
            >
              {provider.id === 'google' ? 'Sign in with Google' : 'Connect account'}
            </button>
          )}
          {!working && accountId.current && failure && !disconnectReview && (
            <button
              className="text-button"
              onClick={() => void discover(accountId.current!, generation.current)}
            >
              Retry discovery
            </button>
          )}
          {discovery && !disconnectReview && (
            <button
              className="primary-button"
              disabled={working || closing}
              onClick={async () => {
                setWorking(true);
                setSaving(true);
                setFailure('');
                try {
                  await calendarClient.commitSelection(discovery.id, selected);
                  onRefresh();
                  await close(true);
                } catch (e) {
                  setFailure(errorMessage(e));
                  setWorking(false);
                  setSaving(false);
                }
              }}
            >
              Save selection
            </button>
          )}
        </div>
        {account && !working && !disconnectReview && (
          <div className="calendar-dialog-actions">
            <button
              className="text-button"
              disabled={!provider.connectionAvailable || closing}
              onClick={() => void start()}
            >
              Reconnect account
            </button>
            <button
              className="text-button"
              disabled={closing}
              onClick={async () => {
                setWorking(true);
                setSaving(true);
                setFailure('');
                try {
                  const review = await calendarClient.reviewDisconnect(account.id);
                  if (review.pendingCount || review.conflictCount) {
                    setDisconnectReview(review);
                    setConfirmDiscard(false);
                    setWorking(false);
                    setSaving(false);
                  } else {
                    await calendarClient.disconnectAccount(account.id);
                    onRefresh();
                    await close(true);
                  }
                } catch (e) {
                  setFailure(errorMessage(e));
                  onRefresh();
                  setWorking(false);
                  setSaving(false);
                }
              }}
            >
              Disconnect account
            </button>
          </div>
        )}
        {account && disconnectReview && (
          <section aria-label="Review pending changes">
            <p>
              This account has {disconnectReview.pendingCount} pending changes and{' '}
              {disconnectReview.conflictCount}{' '}
              {disconnectReview.conflictCount === 1 ? 'conflict' : 'conflicts'} across its
              calendars.
            </p>
            <p>
              Discarding restores the last known Google versions in the cache and removes local
              changes that have not synchronized. This cannot be undone. Reconnect to refresh the
              cache before removing individual calendars from aika.
            </p>
            <p>
              {disconnectReview.attemptedCount} pending changes have already been attempted. Changes
              already sent to Google may still take effect. Disconnecting does not revoke Google
              access or undo changes in Google.
            </p>
            <label>
              <input
                type="checkbox"
                checked={confirmDiscard}
                disabled={working || closing}
                onChange={(e) => setConfirmDiscard(e.target.checked)}
              />{' '}
              Discard these local changes
            </label>
            <div className="calendar-dialog-actions">
              <button
                className="text-button"
                disabled={working || closing}
                onClick={() => {
                  setDisconnectReview(null);
                  setConfirmDiscard(false);
                  setFailure('');
                }}
              >
                Keep changes
              </button>
              <button
                className="primary-button"
                disabled={!confirmDiscard || working || closing}
                onClick={async () => {
                  setWorking(true);
                  setSaving(true);
                  setFailure('');
                  try {
                    await calendarClient.discardAndDisconnect(disconnectReview);
                    onRefresh();
                    await close(true);
                  } catch (e) {
                    setFailure(errorMessage(e));
                    // Refresh also reflects a committed disconnect if credential deletion failed.
                    onRefresh();
                    setDisconnectReview(null);
                    setConfirmDiscard(false);
                    setWorking(false);
                    setSaving(false);
                  }
                }}
              >
                Discard and disconnect
              </button>
            </div>
          </section>
        )}
        {!provider.connectionAvailable && (
          <p role="status" className="provider-reason">
            {provider.unavailableReason}
          </p>
        )}
      </div>
    </SidebarPopup>
  );
}
