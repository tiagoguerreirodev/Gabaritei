import type { JSX } from "solid-js";
import Modal from "./Modal";

export default function ConfirmModal(props: {
  title: string;
  children: JSX.Element;
  confirmLabel: string;
  cancelLabel?: string;
  busyLabel?: string;
  danger?: boolean;
  busy?: boolean;
  dismissBlocked?: boolean;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const close = () => {
    if (!props.dismissBlocked) props.onClose();
  };

  return (
    <Modal
      title={props.title}
      onClose={close}
      footer={
        <>
          <button type="button" disabled={props.dismissBlocked} onClick={close}>
            {props.cancelLabel ?? "Cancelar"}
          </button>
          <button
            type="button"
            class={props.danger ? "primary danger" : "primary"}
            disabled={props.busy}
            onClick={() => props.onConfirm()}
          >
            {props.busy && props.busyLabel ? props.busyLabel : props.confirmLabel}
          </button>
        </>
      }
    >
      {props.children}
    </Modal>
  );
}
