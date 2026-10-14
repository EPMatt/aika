type ShortcutKey = { label: string; name?: string };

export function KeyboardShortcuts() {
  const modifier = /^Mac/i.test(navigator.platform)
    ? { label: 'Cmd', name: 'Command' }
    : { label: 'Ctrl', name: 'Control' };
  const shortcuts: { action: string; keys: ShortcutKey[]; alternatives?: boolean }[] = [
    { action: 'New event', keys: [{ label: 'N' }] },
    { action: 'Go to today', keys: [{ label: 'T' }] },
    { action: 'Month view', keys: [{ label: '1' }] },
    { action: 'Week view', keys: [{ label: '2' }] },
    { action: 'Day view', keys: [{ label: '3' }] },
    { action: 'Undo', keys: [modifier, { label: 'Z' }] },
    { action: 'Edit focused event', keys: [{ label: 'Enter' }] },
    { action: 'Create event on focused date or time slot', keys: [{ label: 'Enter' }] },
    {
      action: 'Navigate month dates',
      keys: [
        { label: '←', name: 'Arrow left' },
        { label: '↑', name: 'Arrow up' },
        { label: '↓', name: 'Arrow down' },
        { label: '→', name: 'Arrow right' },
      ],
      alternatives: true,
    },
    {
      action: 'Dismiss popup/editor or cancel selection',
      keys: [{ label: 'Esc', name: 'Escape' }],
    },
  ];

  return (
    <ul className="shortcut-list" aria-label="Keyboard shortcuts" role="list">
      {shortcuts.map(({ action, keys, alternatives }) => (
        <li className="shortcut-row" key={action}>
          <span>{action}</span>
          <span className="shortcut-keys">
            {keys.map(({ label, name }, index) => (
              <span className="shortcut-key-part" key={label}>
                {index > 0 && (
                  <span className="shortcut-separator">
                    {alternatives ? (
                      <>
                        <span aria-hidden="true">/</span>
                        <span className="sr-only"> or </span>
                      </>
                    ) : (
                      ' + '
                    )}
                  </span>
                )}
                <kbd className="shortcut-key">
                  {name ? (
                    <>
                      <span aria-hidden="true">{label}</span>
                      <span className="sr-only">{name}</span>
                    </>
                  ) : (
                    label
                  )}
                </kbd>
              </span>
            ))}
          </span>
        </li>
      ))}
    </ul>
  );
}
