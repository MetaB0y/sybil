from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.actor_role import ActorRole
from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="ActorIdentityResponse")



@_attrs_define
class ActorIdentityResponse:
    """ 
        Attributes:
            account_id (int):
            principal_id (str):
            ready (bool):
            role (ActorRole):
            last_observed_height (int | None | Unset):
     """

    account_id: int
    principal_id: str
    ready: bool
    role: ActorRole
    last_observed_height: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        principal_id = self.principal_id

        ready = self.ready

        role = self.role.value

        last_observed_height: int | None | Unset
        if isinstance(self.last_observed_height, Unset):
            last_observed_height = UNSET
        else:
            last_observed_height = self.last_observed_height


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "principal_id": principal_id,
            "ready": ready,
            "role": role,
        })
        if last_observed_height is not UNSET:
            field_dict["last_observed_height"] = last_observed_height

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        principal_id = d.pop("principal_id")

        ready = d.pop("ready")

        role = ActorRole(d.pop("role"))




        def _parse_last_observed_height(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        last_observed_height = _parse_last_observed_height(d.pop("last_observed_height", UNSET))


        actor_identity_response = cls(
            account_id=account_id,
            principal_id=principal_id,
            ready=ready,
            role=role,
            last_observed_height=last_observed_height,
        )


        actor_identity_response.additional_properties = d
        return actor_identity_response

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
