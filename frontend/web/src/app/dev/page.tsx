import { redirect } from "next/navigation";

export default function DevIndex() {
  redirect("/dev/overview");
}
