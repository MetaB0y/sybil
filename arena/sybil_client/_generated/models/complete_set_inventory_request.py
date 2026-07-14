from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.complete_set_action_request_type_0 import CompleteSetActionRequestType0
  from ..models.complete_set_action_request_type_1 import CompleteSetActionRequestType1





T = TypeVar("T", bound="CompleteSetInventoryRequest")



@_attrs_define
class CompleteSetInventoryRequest:
    """ 
        Attributes:
            actions (list[CompleteSetActionRequestType0 | CompleteSetActionRequestType1]):
     """

    actions: list[CompleteSetActionRequestType0 | CompleteSetActionRequestType1]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.complete_set_action_request_type_0 import CompleteSetActionRequestType0
        from ..models.complete_set_action_request_type_1 import CompleteSetActionRequestType1
        actions = []
        for actions_item_data in self.actions:
            actions_item: dict[str, Any]
            if isinstance(actions_item_data, CompleteSetActionRequestType0):
                actions_item = actions_item_data.to_dict()
            else:
                actions_item = actions_item_data.to_dict()

            actions.append(actions_item)




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "actions": actions,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.complete_set_action_request_type_0 import CompleteSetActionRequestType0
        from ..models.complete_set_action_request_type_1 import CompleteSetActionRequestType1
        d = dict(src_dict)
        actions = []
        _actions = d.pop("actions")
        for actions_item_data in (_actions):
            def _parse_actions_item(data: object) -> CompleteSetActionRequestType0 | CompleteSetActionRequestType1:
                try:
                    if not isinstance(data, dict):
                        raise TypeError()
                    componentsschemas_complete_set_action_request_type_0 = CompleteSetActionRequestType0.from_dict(data)



                    return componentsschemas_complete_set_action_request_type_0
                except (TypeError, ValueError, AttributeError, KeyError):
                    pass
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_complete_set_action_request_type_1 = CompleteSetActionRequestType1.from_dict(data)



                return componentsschemas_complete_set_action_request_type_1

            actions_item = _parse_actions_item(actions_item_data)

            actions.append(actions_item)


        complete_set_inventory_request = cls(
            actions=actions,
        )


        complete_set_inventory_request.additional_properties = d
        return complete_set_inventory_request

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
