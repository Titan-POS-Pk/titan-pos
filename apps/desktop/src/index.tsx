/* @refresh reload */
/**
 * Application Entry Point
 *
 * This file bootstraps the SolidJS application and mounts it to the DOM.
 *
 * ## Application Structure
 * ```
 * index.tsx (this file)
 *     │
 *     └── App.tsx (main layout)
 *             │
 *             ├── ProductSearch.tsx (search bar + results grid)
 *             │
 *             ├── Cart.tsx (cart display with totals)
 *             │
 *             └── TenderModal.tsx (payment processing)
 * ```
 */

import { render } from 'solid-js/web';
import App from './App';
import './styles/index.css';

// Find the root element
const root = document.getElementById('root');

if (!root) {
  throw new Error('Root element not found. Check index.html for <div id="root"></div>');
}

// Mount the SolidJS app
render(() => <App />, root);
