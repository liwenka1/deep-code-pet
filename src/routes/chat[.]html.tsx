import { createFileRoute } from '@tanstack/react-router'
import { ChatWindow } from '../windows/chat/ChatWindow'

export const Route = createFileRoute('/chat.html')({ ssr: false, component: ChatWindow })
