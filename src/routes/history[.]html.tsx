import { createFileRoute } from '@tanstack/react-router'
import { HistoryWindow } from '../windows/history/HistoryWindow'

export const Route = createFileRoute('/history.html')({ ssr: false, component: HistoryWindow })
