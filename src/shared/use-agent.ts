import { useEffect, useState, useSyncExternalStore } from 'react'
import { AgentController } from '../core/brain/controller'
import { agentPort } from '../platform/desktop'

export function useAgent(withHistory = true) {
  const [controller] = useState(() => new AgentController(agentPort, withHistory))
  const view = useSyncExternalStore(controller.subscribe, controller.getSnapshot, controller.getSnapshot)
  useEffect(() => { void controller.connect(); return () => controller.disconnect() }, [controller])
  return { view, controller }
}
