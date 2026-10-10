import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import './styles.css';
import { desktop } from './client';
import { DateTime } from 'luxon';

// Browser-only review harness. Native launch never loads or seeds fixtures.
const repository = desktop
  ? null
  : await (async () => {
      const { initializeRecurrence } = await import('./recurrence');
      await initializeRecurrence();
      const { PrototypeRepository, sampleEvents } = await import('./prototype');
      const params = new URLSearchParams(location.search);
      const value = DateTime.fromISO(params.get('date') ?? '');
      const scenario = params.get('scenario');
      return new PrototypeRepository(
        sampleEvents(
          value.isValid ? value : DateTime.local(),
          scenario === 'empty' || scenario === 'dense' ? scenario : 'standard',
        ),
      );
    })();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App repository={repository} />
  </React.StrictMode>,
);
