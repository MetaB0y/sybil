from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.admit_timing_view_response import AdmitTimingViewResponse
  from ..models.rejected_order_view_response import RejectedOrderViewResponse
  from ..models.removed_order_view_response import RemovedOrderViewResponse





T = TypeVar("T", bound="DerivedViewSidecarResponse")



@_attrs_define
class DerivedViewSidecarResponse:
    """ 
        Attributes:
            provenance (str): Always `derived_unproven`: this sidecar is sequencer-derived read-model
                data and is not part of the witness, state root, events root, witness
                root, DA commitment, or ZK guest input.
            admits (list[AdmitTimingViewResponse] | Unset): Admission timing rows. `is_new=false` means the order was
                carried from
                a prior block's resting book; `is_new=true` means a distinct admission
                first became visible to this block's view.
            rejection_history (list[RejectedOrderViewResponse] | Unset): Rejection rows that were intentionally mirrored
                into account history.
                Canonical rejections remain in `BlockResponse.rejections`.
            removed_orders (list[RemovedOrderViewResponse] | Unset): Resting orders removed during block production.
                Derived/unproven view
                rows used for analytics and lifecycle displays.
     """

    provenance: str
    admits: list[AdmitTimingViewResponse] | Unset = UNSET
    rejection_history: list[RejectedOrderViewResponse] | Unset = UNSET
    removed_orders: list[RemovedOrderViewResponse] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.admit_timing_view_response import AdmitTimingViewResponse
        from ..models.rejected_order_view_response import RejectedOrderViewResponse
        from ..models.removed_order_view_response import RemovedOrderViewResponse
        provenance = self.provenance

        admits: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.admits, Unset):
            admits = []
            for admits_item_data in self.admits:
                admits_item = admits_item_data.to_dict()
                admits.append(admits_item)



        rejection_history: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.rejection_history, Unset):
            rejection_history = []
            for rejection_history_item_data in self.rejection_history:
                rejection_history_item = rejection_history_item_data.to_dict()
                rejection_history.append(rejection_history_item)



        removed_orders: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.removed_orders, Unset):
            removed_orders = []
            for removed_orders_item_data in self.removed_orders:
                removed_orders_item = removed_orders_item_data.to_dict()
                removed_orders.append(removed_orders_item)




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "provenance": provenance,
        })
        if admits is not UNSET:
            field_dict["admits"] = admits
        if rejection_history is not UNSET:
            field_dict["rejection_history"] = rejection_history
        if removed_orders is not UNSET:
            field_dict["removed_orders"] = removed_orders

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.admit_timing_view_response import AdmitTimingViewResponse
        from ..models.rejected_order_view_response import RejectedOrderViewResponse
        from ..models.removed_order_view_response import RemovedOrderViewResponse
        d = dict(src_dict)
        provenance = d.pop("provenance")

        _admits = d.pop("admits", UNSET)
        admits: list[AdmitTimingViewResponse] | Unset = UNSET
        if _admits is not UNSET:
            admits = []
            for admits_item_data in _admits:
                admits_item = AdmitTimingViewResponse.from_dict(admits_item_data)



                admits.append(admits_item)


        _rejection_history = d.pop("rejection_history", UNSET)
        rejection_history: list[RejectedOrderViewResponse] | Unset = UNSET
        if _rejection_history is not UNSET:
            rejection_history = []
            for rejection_history_item_data in _rejection_history:
                rejection_history_item = RejectedOrderViewResponse.from_dict(rejection_history_item_data)



                rejection_history.append(rejection_history_item)


        _removed_orders = d.pop("removed_orders", UNSET)
        removed_orders: list[RemovedOrderViewResponse] | Unset = UNSET
        if _removed_orders is not UNSET:
            removed_orders = []
            for removed_orders_item_data in _removed_orders:
                removed_orders_item = RemovedOrderViewResponse.from_dict(removed_orders_item_data)



                removed_orders.append(removed_orders_item)


        derived_view_sidecar_response = cls(
            provenance=provenance,
            admits=admits,
            rejection_history=rejection_history,
            removed_orders=removed_orders,
        )


        derived_view_sidecar_response.additional_properties = d
        return derived_view_sidecar_response

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
