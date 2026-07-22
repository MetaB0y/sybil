from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.api_error_response import ApiErrorResponse
from ...models.bridge_withdrawal_response import BridgeWithdrawalResponse
from typing import cast



def _get_kwargs(
    id: int,

) -> dict[str, Any]:
    

    

    

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/accounts/{id}/withdrawals".format(id=quote(str(id), safe=""),),
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | ApiErrorResponse | list[BridgeWithdrawalResponse] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in (_response_200):
            response_200_item = BridgeWithdrawalResponse.from_dict(response_200_item_data)



            response_200.append(response_200_item)

        return response_200

    if response.status_code == 400:
        response_400 = ApiErrorResponse.from_dict(response.json())



        return response_400

    if response.status_code == 401:
        response_401 = cast(Any, None)
        return response_401

    if response.status_code == 403:
        response_403 = cast(Any, None)
        return response_403

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | ApiErrorResponse | list[BridgeWithdrawalResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: int,
    *,
    client: AuthenticatedClient,

) -> Response[Any | ApiErrorResponse | list[BridgeWithdrawalResponse]]:
    """ GET /v1/accounts/{id}/withdrawals

     Returns the account's currently active withdrawal leaves. Terminal leaves
    are visible with their terminal status until the next committed block
    retires them, then disappear from this collection. Historical creation
    blocks remain immutable and must not be used as a current-status view.

    Args:
        id (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse | list[BridgeWithdrawalResponse]]
     """


    kwargs = _get_kwargs(
        id=id,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    id: int,
    *,
    client: AuthenticatedClient,

) -> Any | ApiErrorResponse | list[BridgeWithdrawalResponse] | None:
    """ GET /v1/accounts/{id}/withdrawals

     Returns the account's currently active withdrawal leaves. Terminal leaves
    are visible with their terminal status until the next committed block
    retires them, then disappear from this collection. Historical creation
    blocks remain immutable and must not be used as a current-status view.

    Args:
        id (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse | list[BridgeWithdrawalResponse]
     """


    return sync_detailed(
        id=id,
client=client,

    ).parsed

async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient,

) -> Response[Any | ApiErrorResponse | list[BridgeWithdrawalResponse]]:
    """ GET /v1/accounts/{id}/withdrawals

     Returns the account's currently active withdrawal leaves. Terminal leaves
    are visible with their terminal status until the next committed block
    retires them, then disappear from this collection. Historical creation
    blocks remain immutable and must not be used as a current-status view.

    Args:
        id (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse | list[BridgeWithdrawalResponse]]
     """


    kwargs = _get_kwargs(
        id=id,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient,

) -> Any | ApiErrorResponse | list[BridgeWithdrawalResponse] | None:
    """ GET /v1/accounts/{id}/withdrawals

     Returns the account's currently active withdrawal leaves. Terminal leaves
    are visible with their terminal status until the next committed block
    retires them, then disappear from this collection. Historical creation
    blocks remain immutable and must not be used as a current-status view.

    Args:
        id (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse | list[BridgeWithdrawalResponse]
     """


    return (await asyncio_detailed(
        id=id,
client=client,

    )).parsed
