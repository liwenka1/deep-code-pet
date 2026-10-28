import { createFileRoute } from '@tanstack/react-router'
import { PetWindow } from '../windows/pet/PetWindow'

export const Route = createFileRoute('/')({ ssr: false, component: PetWindow })
